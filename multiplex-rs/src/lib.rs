extern crate bytes;
extern crate futures;
extern crate libp2p_stream_muxer;
extern crate tokio_io;
extern crate varint;
extern crate num_bigint;
extern crate num_traits;
extern crate parking_lot;

use bytes::Bytes;
use futures::prelude::*;
use libp2p_stream_muxer::StreamMuxer;
use parking_lot::Mutex;
use std::collections::{HashSet, HashMap};
use std::io::{self, Read, Write};
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};

// So the multiplex is essentially a distributed finite state machine.
//
// In the first state the header must be read so that we know which substream to hand off the
// upcoming packet to. This is first-come, first-served - whichever substream begins reading the
// packet will be locked into reading the header until it is consumed (this may be changed in the
// future, for example by allowing the streams to cooperate on parsing headers). This implementation
// of `Multiplex` operates under the assumption that all substreams are consumed relatively equally.
// A higher-level wrapper may wrap this and add some level of buffering.
//
// In the second state, the substream ID is known. Only this substream can progress until the packet
// is consumed.

/// Number of bits used for the metadata on multiplex packets
enum NextMultiplexState {
    NewStream(usize),
    ParsingMessageBody(usize),
    Ignore,
}

enum MultiplexState {
    Header { state: varint::DecoderState },
    BodyLength {
        state: varint::DecoderState,
        next: NextMultiplexState,
    },
    NewStream {
        substream_id: usize,
        name: bytes::BytesMut,
        remaining_bytes: usize,
    },
    ParsingMessageBody {
        substream_id: usize,
        remaining_bytes: usize,
    },
    Ignore { remaining_bytes: usize },
}

impl Default for MultiplexState {
    fn default() -> Self {
        MultiplexState::Header { state: Default::default() }
    }
}

// TODO: Add writing. We should also add some form of "pending packet" so that we can always open at
//       least one new substream. If this is stored on the substream itself then we can open
//       infinite new substreams.
//
//       When we've implemented writing, we should send the close message on `Substream` drop. This
//       should probably be implemented with some kind of "pending close message" queue. The
//       priority should go:
//       1. Open new stream messages
//       2. Regular messages
//       3. Close messages
//       Since if we receive a message to a closed stream we just drop it anyway.
struct MultiplexShared<T> {
    // We use `Option` in order to take ownership of heap allocations within `DecoderState` and
    // `BytesMut`. If this is ever observably `None` then something has panicked and the `Mutex`
    // will be poisoned.
    read_state: Option<MultiplexState>,
    stream: T,
    // true if the stream is open, false otherwise
    open_streams: HashMap<usize, bool>,
    // TODO: Should we use a version of this with a fixed size that doesn't allocate and return
    //       `WouldBlock` if it's full?
    to_open: HashMap<usize, Bytes>,
}

pub struct Substream<T> {
    id: usize,
    name: Option<Bytes>,
    state: Arc<Mutex<MultiplexShared<T>>>,
}

impl<T> Drop for Substream<T> {
    fn drop(&mut self) {
        let mut lock = self.state.lock();

        lock.open_streams.insert(self.id, false);
    }
}

impl<T> Substream<T> {
    fn new<B: Into<Option<Bytes>>>(
        id: usize,
        name: B,
        state: Arc<Mutex<MultiplexShared<T>>>,
    ) -> Self {
        let name = name.into();

        Substream { id, name, state }
    }

    pub fn name(&self) -> Option<&Bytes> {
        self.name.as_ref()
    }
}

/// This is unsafe because you must ensure that only the `AsyncRead` that was passed in is later
/// used to write to the returned buffer.
unsafe fn create_buffer_for<R: AsyncRead>(capacity: usize, inner: &R) -> bytes::BytesMut {
    let mut buffer = bytes::BytesMut::with_capacity(capacity);
    buffer.set_len(capacity);
    inner.prepare_uninitialized_buffer(&mut buffer);
    buffer
}

fn read_stream<T: AsyncRead>(
    mut stream_data: Option<(usize, &mut [u8])>,
    lock: &mut MultiplexShared<T>,
) -> io::Result<usize> {
    use num_traits::cast::ToPrimitive;
    use MultiplexState::*;

    let stream_has_been_gracefully_closed = stream_data
        .as_ref()
        .and_then(|&(id, _)| lock.open_streams.get(&id))
        .map(|is_open| !is_open)
        .unwrap_or(false);

    let mut on_block: io::Result<usize> = if stream_has_been_gracefully_closed {
        Ok(0)
    } else {
        Err(io::Error::from(io::ErrorKind::WouldBlock))
    };

    loop {
        match lock.read_state.take().expect("Logic error or panic") {
            Header { state: varint_state } => {
                match varint_state.read(&mut lock.stream).map_err(|_| {
                    io::Error::from(io::ErrorKind::Other)
                })? {
                    Ok(header) => {
                        let MultiplexHeader {
                            substream_id,
                            packet_type,
                        } = MultiplexHeader::parse(header).map_err(|_| {
                            io::Error::from(io::ErrorKind::Other)
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
                            PacketType::Close(_) |
                            PacketType::Reset(_) => {
                                lock.read_state = Some(BodyLength {
                                    state: Default::default(),
                                    next: NextMultiplexState::Ignore,
                                });

                                lock.open_streams.remove(&substream_id);
                            }
                        }
                    }
                    Err(new_state) => {
                        lock.read_state = Some(Header { state: new_state });
                        return on_block;
                    }
                }
            }
            BodyLength {
                state: varint_state,
                next,
            } => {
                match varint_state.read(&mut lock.stream).map_err(|_| {
                    io::Error::from(io::ErrorKind::Other)
                })? {
                    Ok(length) => {
                        use NextMultiplexState::*;

                        let length = length.to_usize().ok_or(
                            io::Error::from(io::ErrorKind::Other),
                        )?;

                        lock.read_state = Some(match next {
                            Ignore => MultiplexState::Ignore { remaining_bytes: length },
                            NewStream(substream_id) => MultiplexState::NewStream {
                                // This is safe as long as we only use `lock.stream` to write to
                                // this field
                                name: unsafe { create_buffer_for(length, &lock.stream) },
                                remaining_bytes: length,
                                substream_id,
                            },
                            ParsingMessageBody(substream_id) => {
                                let is_open = lock.open_streams
                                    .get(&substream_id)
                                    .map(|is_open| *is_open)
                                    .unwrap_or(false);
                                if is_open {
                                    MultiplexState::ParsingMessageBody {
                                        remaining_bytes: length,
                                        substream_id,
                                    }
                                } else {
                                    MultiplexState::Ignore { remaining_bytes: length }
                                }
                            }
                        });
                    }
                    Err(new_state) => {
                        lock.read_state = Some(BodyLength {
                            state: new_state,
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
                    lock.to_open.insert(substream_id, name.freeze());

                    lock.read_state = Some(Default::default());
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
                            })
                        }
                        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                            lock.read_state = Some(NewStream {
                                substream_id,
                                name,
                                remaining_bytes,
                            });

                            return on_block;
                        }
                        Err(other) => return Err(other),
                    }
                }
            }
            ParsingMessageBody {
                substream_id,
                remaining_bytes,
            } => {
                if let Some((ref mut id, ref mut buf)) = stream_data {
                    use MultiplexState::*;

                    if substream_id == *id {
                        if remaining_bytes == 0 {
                            lock.read_state = Some(Default::default());
                        } else {
                            let read_result = {
                                let new_len = buf.len().min(remaining_bytes);
                                let slice = &mut buf[..new_len];

                                lock.stream.read(slice)
                            };

                            match read_result {
                                Ok(consumed) => {
                                    let new_remaining = remaining_bytes - consumed;

                                    lock.read_state = Some(ParsingMessageBody {
                                        substream_id,
                                        remaining_bytes: new_remaining,
                                    });

                                    on_block = Ok(on_block.unwrap_or(0) + consumed);
                                }
                                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                                    lock.read_state = Some(ParsingMessageBody {
                                        substream_id,
                                        remaining_bytes,
                                    });

                                    return on_block;
                                }
                                Err(other) => return Err(other),
                            }
                        }
                    } else {
                        lock.read_state = Some(ParsingMessageBody {
                            substream_id,
                            remaining_bytes,
                        });

                        // We cannot make progress here, another stream has to accept this packet
                        return on_block;
                    }
                }
            }
            Ignore { mut remaining_bytes } => {
                let mut ignore_buf: [u8; 256] = [0; 256];

                loop {
                    if remaining_bytes == 0 {
                        lock.read_state = Some(Default::default());
                    } else {
                        let new_len = ignore_buf.len().min(remaining_bytes);
                        match lock.stream.read(&mut ignore_buf[..new_len]) {
                            Ok(consumed) => remaining_bytes -= consumed,
                            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                                lock.read_state = Some(Ignore { remaining_bytes });

                                return on_block;
                            }
                            Err(other) => return Err(other),
                        }
                    }
                }
            }
        }
    }
}

// TODO: We always zero the buffer, we should delegate to the inner stream. Maybe use a `RWLock`
//       instead?
impl<T: AsyncRead> Read for Substream<T> {
    // TODO: Is it wasteful to have all of our substreams try to make progress? Can we use an
    //       `AtomicBool` or `AtomicUsize` to limit the substreams that try to progress?
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut lock = match self.state.try_lock() {
            Some(lock) => lock,
            None => return Err(io::Error::from(io::ErrorKind::WouldBlock)),
        };

        read_stream(Some((self.id, buf)), &mut lock)
    }
}

impl<T: AsyncRead> AsyncRead for Substream<T> {}

impl<T: AsyncWrite> Write for Substream<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unimplemented!()
    }

    fn flush(&mut self) -> io::Result<()> {
        unimplemented!()
    }
}

impl<T: AsyncWrite> AsyncWrite for Substream<T> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        unimplemented!()
    }
}

struct ParseError;

enum MultiplexEnd {
    Initiator,
    Receiver,
}

struct MultiplexHeader {
    pub packet_type: PacketType,
    pub substream_id: usize,
}
enum PacketType {
    Open,
    Close(MultiplexEnd),
    Reset(MultiplexEnd),
    Message(MultiplexEnd),
}

impl MultiplexHeader {
    // TODO: Use `u128` or another large integer type instead of bigint since we never use more than
    //       `pointer width + FLAG_BITS` bits and unconditionally allocating 1-3 `u32`s for that is
    //       ridiculous (especially since even for small numbers we have to allocate 1 `u32`).
    //       If this is the future and `BigUint` is better-optimised (maybe by using `Bytes`) then
    //       forget it.
    fn parse(header: num_bigint::BigUint) -> Result<MultiplexHeader, ParseError> {
        use num_traits::cast::ToPrimitive;

        const FLAG_BITS: usize = 3;

        // `&header` to make `>>` produce a new `BigUint` instead of consuming the old `BigUint`
        let substream_id = ((&header) >> FLAG_BITS).to_usize().ok_or(ParseError)?;

        let flag_mask = (2usize << FLAG_BITS) - 1;
        let flags = header.to_usize().ok_or(ParseError)? & flag_mask;

        // Yes, this is really how it works. No, I don't know why.
        let packet_type = match flags {
            0 => PacketType::Open,

            1 => PacketType::Message(MultiplexEnd::Receiver),
            2 => PacketType::Message(MultiplexEnd::Initiator),

            3 => PacketType::Close(MultiplexEnd::Receiver),
            4 => PacketType::Close(MultiplexEnd::Initiator),

            5 => PacketType::Reset(MultiplexEnd::Receiver),
            6 => PacketType::Reset(MultiplexEnd::Initiator),

            _ => return Err(ParseError),
        };

        Ok(MultiplexHeader {
            substream_id,
            packet_type,
        })
    }
}

pub struct Multiplex<T> {
    state: Arc<Mutex<MultiplexShared<T>>>,
}

pub struct InboundStream<T> {
    state: Arc<Mutex<MultiplexShared<T>>>,
}

impl<T: AsyncRead> Stream for InboundStream<T> {
    type Item = Substream<T>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut lock = match self.state.try_lock() {
            Some(lock) => lock,
            None => return Ok(Async::NotReady),
        };

        // Attempt to make progress, but don't block if we can't
        match read_stream(None, &mut lock) {
            Ok(_) => (),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => (),
            Err(err) => return Err(err),
        }

        let id = if let Some((id, _)) = lock.to_open.iter().next() {
            *id
        } else {
            return Ok(Async::NotReady);
        };

        let name = lock.to_open.remove(&id).expect(
            "We just checked that this key exists and we have exclusive access to the map, QED",
        );

        Ok(Async::Ready(
            Some(Substream::new(id, name, self.state.clone())),
        ))
    }
}

impl<T: AsyncRead + AsyncWrite> StreamMuxer for Multiplex<T> {
    type Substream = Substream<T>;
    type OutboundSubstreams = Box<Stream<Item = Self::Substream, Error = io::Error>>;
    type InboundSubstreams = InboundStream<T>;

    fn inbound(&mut self) -> Self::InboundSubstreams {
        InboundStream { state: self.state.clone() }
    }

    fn outbound(&mut self) -> Self::OutboundSubstreams {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
