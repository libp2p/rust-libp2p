// Copyright 2017 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use shared::{ByteBuf, MultiplexShared, SubstreamMetadata};
use header::MultiplexHeader;

use varint;
use futures::task;
use std::io;
use tokio_io::AsyncWrite;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RequestType {
    Meta,
    Substream,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct WriteRequest {
    header: MultiplexHeader,
    request_type: RequestType,
}

impl WriteRequest {
    pub fn substream(header: MultiplexHeader) -> Self {
        WriteRequest {
            header,
            request_type: RequestType::Substream,
        }
    }

    pub fn meta(header: MultiplexHeader) -> Self {
        WriteRequest {
            header,
            request_type: RequestType::Meta,
        }
    }
}

#[derive(Default, Debug)]
pub struct MultiplexWriteState {
    current: Option<(WriteRequest, MultiplexWriteStateInner)>,
    queued: Option<WriteRequest>,
    // TODO: Actually close these
    to_close: Vec<u32>,
}

#[derive(Debug)]
pub enum MultiplexWriteStateInner {
    WriteHeader { state: varint::EncoderState<u64> },
    BodyLength { state: varint::EncoderState<usize> },
    Body { size: usize },
}

pub fn write_stream<T: AsyncWrite>(
    lock: &mut MultiplexShared<T>,
    write_request: WriteRequest,
    buf: &mut io::Cursor<ByteBuf>,
) -> io::Result<usize> {
    use futures::Async;
    use num_traits::cast::ToPrimitive;
    use varint::WriteState;
    use write::MultiplexWriteStateInner::*;

    let mut on_block = Err(io::ErrorKind::WouldBlock.into());
    let mut write_state = lock.write_state.take().unwrap_or_default();
    let (request, mut state) = write_state.current.take().unwrap_or_else(|| {
        (
            write_request,
            MultiplexWriteStateInner::WriteHeader {
                state: varint::EncoderState::new(write_request.header.to_u64()),
            },
        )
    });

    let id = write_request.header.substream_id;

    match (request.request_type, write_request.request_type) {
        (RequestType::Substream, RequestType::Substream) if request.header.substream_id != id => {
            use std::mem;

            if let Some(cur) = lock.open_streams
                .entry(id)
                .or_insert_with(|| SubstreamMetadata::new_open())
                .write_tasks_mut()
            {
                cur.push(task::current());
            }

            if let Some(tasks) = lock.open_streams
                .get_mut(&request.header.substream_id)
                .and_then(SubstreamMetadata::write_tasks_mut)
                .map(|cur| mem::replace(cur, Default::default()))
            {
                for task in tasks {
                    task.notify();
                }
            }

            lock.write_state = Some(write_state);
            return on_block;
        }
        (RequestType::Substream, RequestType::Meta) => {
            use std::mem;

            lock.write_state = Some(write_state);
            lock.meta_write_tasks.push(task::current());

            if let Some(tasks) = lock.open_streams
                .get_mut(&request.header.substream_id)
                .and_then(SubstreamMetadata::write_tasks_mut)
                .map(|cur| mem::replace(cur, Default::default()))
            {
                for task in tasks {
                    task.notify();
                }
            }

            return on_block;
        }
        (RequestType::Meta, RequestType::Substream) => {
            use std::mem;

            lock.write_state = Some(write_state);

            if let Some(cur) = lock.open_streams
                .entry(id)
                .or_insert_with(|| SubstreamMetadata::new_open())
                .write_tasks_mut()
            {
                cur.push(task::current());
            }

            for task in mem::replace(&mut lock.meta_write_tasks, Default::default()) {
                task.notify();
            }

            return on_block;
        }
        _ => {}
    }

    loop {
        // Err = should return, Ok = continue
        let new_state = match state {
            WriteHeader {
                state: mut inner_state,
            } => match inner_state
                .write(&mut lock.stream)
                .map_err(|_| io::ErrorKind::Other)?
            {
                Async::Ready(WriteState::Done(_)) => Ok(BodyLength {
                    state: varint::EncoderState::new(buf.get_ref().len()),
                }),
                Async::Ready(WriteState::Pending(_)) | Async::NotReady => {
                    Err(Some(WriteHeader { state: inner_state }))
                }
            },
            BodyLength {
                state: mut inner_state,
            } => match inner_state
                .write(&mut lock.stream)
                .map_err(|_| io::ErrorKind::Other)?
            {
                Async::Ready(WriteState::Done(_)) => Ok(Body {
                    size: inner_state.source().to_usize().unwrap_or(::std::usize::MAX),
                }),
                Async::Ready(WriteState::Pending(_)) => Ok(BodyLength { state: inner_state }),
                Async::NotReady => Err(Some(BodyLength { state: inner_state })),
            },
            Body { size } => {
                if buf.position() == buf.get_ref().len() as u64 {
                    Err(None)
                } else {
                    match lock.stream.write(&buf.get_ref()[buf.position() as usize..]) {
                        Ok(just_written) => {
                            let cur_pos = buf.position();
                            buf.set_position(cur_pos + just_written as u64);
                            on_block = Ok(on_block.unwrap_or(0) + just_written);
                            Ok(Body {
                                size: size - just_written,
                            })
                        }
                        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                            Err(Some(Body { size }))
                        }
                        Err(other) => {
                            return Err(other);
                        }
                    }
                }
            }
        };

        match new_state {
            Ok(new_state) => state = new_state,
            Err(new_state) => {
                write_state.current = new_state.map(|state| (request, state));
                lock.write_state = Some(write_state);
                return on_block;
            }
        }
    }
}
