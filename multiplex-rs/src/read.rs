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

use {bytes, varint};
use futures::Async;
use futures::task;
use header::{MultiplexHeader, PacketType};
use std::io;
use tokio_io::AsyncRead;
use shared::SubstreamMetadata;
use circular_buffer::Array;

pub enum NextMultiplexState {
    NewStream(u32),
    ParsingMessageBody(u32),
    Ignore,
}

pub enum MultiplexReadState {
    Header {
        state: varint::DecoderState<u64>,
    },
    BodyLength {
        state: varint::DecoderState<usize>,
        next: NextMultiplexState,
    },
    NewStream {
        substream_id: u32,
        name: bytes::BytesMut,
        remaining_bytes: usize,
    },
    ParsingMessageBody {
        substream_id: u32,
        remaining_bytes: usize,
    },
    Ignore {
        remaining_bytes: usize,
    },
}

impl Default for MultiplexReadState {
    fn default() -> Self {
        MultiplexReadState::Header {
            state: Default::default(),
        }
    }
}

fn create_buffer(capacity: usize) -> bytes::BytesMut {
    let mut buffer = bytes::BytesMut::with_capacity(capacity);
    let zeroes = [0; 1024];
    let mut cap = capacity;

    while cap > 0 {
        let len = cap.min(zeroes.len());
        buffer.extend_from_slice(&zeroes[..len]);
        cap -= len;
    }

    buffer
}

fn block_on_wrong_stream<T: AsyncRead, Buf: Array<Item = u8>>(
    substream_id: u32,
    remaining_bytes: usize,
    lock: &mut ::shared::MultiplexShared<T, Buf>,
) -> io::Result<()> {
    use std::{mem, slice};

    lock.read_state = Some(MultiplexReadState::ParsingMessageBody {
        substream_id,
        remaining_bytes,
    });

    if let Some((tasks, cache)) = lock.open_streams
        .entry(substream_id)
        .or_insert_with(|| SubstreamMetadata::new_open())
        .open_meta_mut()
        .map(|cur| {
            (
                mem::replace(&mut cur.read, Default::default()),
                &mut cur.read_cache,
            )
        }) {
        // We check `cache.capacity()` since that can totally statically remove this branch in the
        // `== 0` path.
        if cache.capacity() > 0 && cache.len() < cache.capacity() {
            let mut buf: Buf = unsafe { mem::uninitialized() };

            // Can't fail because `cache.len() >= 0`,
            // `cache.len() <= cache.capacity()` and
            // `cache.capacity() == mem::size_of::<Buf>()`
            let buf_prefix = unsafe {
                let max_that_fits_in_buffer = cache.capacity() - cache.len();
                // We know this won't panic because of the earlier
                // `number_read >= buf.len()` check
                let new_len = max_that_fits_in_buffer.min(remaining_bytes);

                slice::from_raw_parts_mut(buf.ptr_mut(), new_len)
            };

            match lock.stream.read(buf_prefix) {
                Ok(consumed) => {
                    let new_remaining = remaining_bytes - consumed;

                    assert!(cache.extend_from_slice(&buf_prefix[..consumed]));

                    lock.read_state = Some(MultiplexReadState::ParsingMessageBody {
                        substream_id,
                        remaining_bytes: new_remaining,
                    });
                }
                Err(err) => {
                    if err.kind() != io::ErrorKind::WouldBlock {
                        for task in tasks {
                            task.notify();
                        }

                        return Err(err);
                    }
                }
            }
        }

        for task in tasks {
            task.notify();
        }
    }

    Ok(())
}

pub fn read_stream<
    'a,
    Buf: Array<Item = u8>,
    O: Into<Option<(u32, &'a mut [u8])>>,
    T: AsyncRead,
>(
    lock: &mut ::shared::MultiplexShared<T, Buf>,
    stream_data: O,
) -> io::Result<usize> {
    read_stream_internal(lock, stream_data.into())
}

fn read_stream_internal<T: AsyncRead, Buf: Array<Item = u8>>(
    lock: &mut ::shared::MultiplexShared<T, Buf>,
    mut stream_data: Option<(u32, &mut [u8])>,
) -> io::Result<usize> {
    use self::MultiplexReadState::*;

    // This is only true if a stream exists and it has been closed in a "graceful" manner, so we
    // can return `Ok(0)` like the `Read` trait requests. In any other case we want to return
    // `WouldBlock`
    let stream_has_been_gracefully_closed = stream_data
        .as_ref()
        .and_then(|&(id, _)| lock.open_streams.get(&id))
        .map(|meta| !meta.open())
        .unwrap_or(false);

    let mut on_block: io::Result<usize> = if stream_has_been_gracefully_closed {
        Ok(0)
    } else {
        Err(io::ErrorKind::WouldBlock.into())
    };

    if let Some((ref mut id, ref mut buf)) = stream_data {
        if let Some(cur) = lock.open_streams
            .entry(*id)
            .or_insert_with(|| SubstreamMetadata::new_open())
            .open_meta_mut()
        {
            cur.read.push(task::current());

            let cache = &mut cur.read_cache;

            if !cache.is_empty() {
                let mut consumed = 0;
                loop {
                    let cur_buf = &mut buf[consumed..];
                    if cur_buf.is_empty() {
                        break;
                    }

                    if let Some(out) = cache.pop_first_n_leaky(cur_buf.len()) {
                        cur_buf[..out.len()].copy_from_slice(out);
                        consumed += out.len();
                    } else {
                        break;
                    };
                }

                on_block = Ok(consumed);
            }
        }
    }

    loop {
        match lock.read_state.take().unwrap_or_default() {
            Header {
                state: mut varint_state,
            } => {
                match varint_state.read(&mut lock.stream) {
                    Ok(Async::Ready(header)) => {
                        let header = if let Some(header) = header {
                            header
                        } else {
                            return Ok(on_block.unwrap_or(0));
                        };

                        let MultiplexHeader {
                            substream_id,
                            packet_type,
                        } = MultiplexHeader::parse(header).map_err(|err| {
                            io::Error::new(
                                io::ErrorKind::Other,
                                format!("Error parsing header: {:?}", err),
                            )
                        })?;

                        match packet_type {
                            PacketType::Open => {
                                lock.read_state = Some(BodyLength {
                                    state: Default::default(),
                                    next: NextMultiplexState::NewStream(substream_id),
                                })
                            }
                            PacketType::Message(_) => {
                                lock.read_state = Some(BodyLength {
                                    state: Default::default(),
                                    next: NextMultiplexState::ParsingMessageBody(substream_id),
                                })
                            }
                            // NOTE: What's the difference between close and reset?
                            PacketType::Close(_) | PacketType::Reset(_) => {
                                lock.read_state = Some(BodyLength {
                                    state: Default::default(),
                                    next: NextMultiplexState::Ignore,
                                });

                                lock.close_stream(substream_id);
                            }
                        }
                    }
                    Ok(Async::NotReady) => {
                        lock.read_state = Some(Header {
                            state: varint_state,
                        });
                        return on_block;
                    }
                    Err(error) => {
                        return if let varint::Error(varint::ErrorKind::Io(inner), ..) = error {
                            Err(inner)
                        } else {
                            Err(io::Error::new(io::ErrorKind::Other, error.description()))
                        };
                    }
                }
            }
            BodyLength {
                state: mut varint_state,
                next,
            } => {
                use self::NextMultiplexState::*;

                match varint_state
                    .read(&mut lock.stream)
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "Error reading varint"))?
                {
                    Async::Ready(length) => {
                        // TODO: Limit `length` to prevent resource-exhaustion DOS
                        let length = if let Some(length) = length {
                            length
                        } else {
                            return Ok(on_block.unwrap_or(0));
                        };

                        lock.read_state = match next {
                            Ignore => Some(MultiplexReadState::Ignore {
                                remaining_bytes: length,
                            }),
                            NewStream(substream_id) => {
                                if length == 0 {
                                    lock.to_open.insert(substream_id, None);

                                    None
                                } else {
                                    Some(MultiplexReadState::NewStream {
                                        // TODO: Uninit buffer
                                        name: create_buffer(length),
                                        remaining_bytes: length,
                                        substream_id,
                                    })
                                }
                            }
                            ParsingMessageBody(substream_id) => {
                                let is_open = lock.open_streams
                                    .get(&substream_id)
                                    .map(SubstreamMetadata::open)
                                    .unwrap_or_else(|| lock.to_open.contains_key(&substream_id));

                                if is_open {
                                    Some(MultiplexReadState::ParsingMessageBody {
                                        remaining_bytes: length,
                                        substream_id,
                                    })
                                } else {
                                    Some(MultiplexReadState::Ignore {
                                        remaining_bytes: length,
                                    })
                                }
                            }
                        };
                    }
                    Async::NotReady => {
                        lock.read_state = Some(BodyLength {
                            state: varint_state,
                            next,
                        });

                        return on_block;
                    }
                }
            }
            NewStream {
                substream_id,
                mut name,
                remaining_bytes,
            } => {
                if remaining_bytes == 0 {
                    lock.to_open.insert(substream_id, Some(name.freeze()));

                    lock.read_state = None;
                } else {
                    let cursor_pos = name.len() - remaining_bytes;
                    let consumed = lock.stream.read(&mut name[cursor_pos..]);

                    match consumed {
                        Ok(consumed) => {
                            let new_remaining = remaining_bytes - consumed;

                            lock.read_state = Some(NewStream {
                                substream_id,
                                name,
                                remaining_bytes: new_remaining,
                            });
                        }
                        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                            lock.read_state = Some(NewStream {
                                substream_id,
                                name,
                                remaining_bytes,
                            });

                            return on_block;
                        }
                        Err(other) => {
                            lock.read_state = Some(NewStream {
                                substream_id,
                                name,
                                remaining_bytes,
                            });

                            return Err(other);
                        }
                    }
                }
            }
            ParsingMessageBody {
                substream_id,
                remaining_bytes,
            } => {
                if let Some((ref mut id, ref mut buf)) = stream_data {
                    use MultiplexReadState::*;

                    let number_read = *on_block.as_ref().unwrap_or(&0);

                    if remaining_bytes == 0 {
                        lock.read_state = None;
                    } else if substream_id == *id {
                        if number_read >= buf.len() {
                            lock.read_state = Some(ParsingMessageBody {
                                substream_id,
                                remaining_bytes,
                            });

                            return Ok(number_read);
                        }

                        let read_result = {
                            // We know this won't panic because of the earlier
                            // `number_read >= buf.len()` check
                            let new_len = (buf.len() - number_read).min(remaining_bytes);
                            let slice = &mut buf[number_read..number_read + new_len];

                            lock.stream.read(slice)
                        };

                        lock.read_state = Some(ParsingMessageBody {
                            substream_id,
                            remaining_bytes,
                        });

                        match read_result {
                            Ok(consumed) => {
                                let new_remaining = remaining_bytes - consumed;

                                lock.read_state = Some(ParsingMessageBody {
                                    substream_id,
                                    remaining_bytes: new_remaining,
                                });

                                on_block = Ok(number_read + consumed);
                            }
                            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                                return on_block;
                            }
                            Err(other) => {
                                return Err(other);
                            }
                        }
                    } else {
                        // We cannot make progress here, another stream has to accept this packet
                        block_on_wrong_stream(substream_id, remaining_bytes, lock)?;

                        return on_block;
                    }
                } else {
                    // We cannot make progress here, another stream has to accept this packet
                    block_on_wrong_stream(substream_id, remaining_bytes, lock)?;

                    return on_block;
                }
            }
            Ignore {
                mut remaining_bytes,
            } => {
                let mut ignore_buf: [u8; 256] = [0; 256];

                loop {
                    if remaining_bytes == 0 {
                        lock.read_state = None;
                        break;
                    } else {
                        let new_len = ignore_buf.len().min(remaining_bytes);
                        match lock.stream.read(&mut ignore_buf[..new_len]) {
                            Ok(consumed) => {
                                remaining_bytes -= consumed;
                                lock.read_state = Some(Ignore {
                                    remaining_bytes: remaining_bytes,
                                });
                            }
                            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                                lock.read_state = Some(Ignore { remaining_bytes });

                                return on_block;
                            }
                            Err(other) => {
                                lock.read_state = Some(Ignore { remaining_bytes });

                                return Err(other);
                            }
                        }
                    }
                }
            }
        }
    }
}
