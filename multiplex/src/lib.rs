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

extern crate arrayvec;
extern crate bytes;
extern crate circular_buffer;
#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_mutex;
extern crate libp2p_swarm as swarm;
extern crate num_bigint;
extern crate num_traits;
extern crate parking_lot;
extern crate rand;
extern crate tokio_io;
extern crate varint;

mod read;
mod write;
mod shared;
mod header;

use bytes::Bytes;
use circular_buffer::Array;
use futures::{Async, Future, Poll};
use futures::future::{self, FutureResult};
use header::MultiplexHeader;
use swarm::muxing::StreamMuxer;
use swarm::{ConnectionUpgrade, Endpoint, Multiaddr};
use futures_mutex::Mutex;
use read::{read_stream, MultiplexReadState};
use shared::{buf_from_slice, ByteBuf, MultiplexShared};
use std::iter;
use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicUsize};
use tokio_io::{AsyncRead, AsyncWrite};
use write::write_stream;

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

pub struct Substream<T, Buf: Array = [u8; 0]> {
    id: u32,
    end: Endpoint,
    name: Option<Bytes>,
    state: Arc<Mutex<MultiplexShared<T, Buf>>>,
    buffer: Option<io::Cursor<ByteBuf>>,
}

impl<T, Buf: Array> Drop for Substream<T, Buf> {
    fn drop(&mut self) {
        let mut lock = self.state.lock().wait().expect("This should never fail");

        lock.close_stream(self.id);
    }
}

impl<T, Buf: Array> Substream<T, Buf> {
    fn new<B: Into<Option<Bytes>>>(
        id: u32,
        end: Endpoint,
        name: B,
        state: Arc<Mutex<MultiplexShared<T, Buf>>>,
    ) -> Self {
        let name = name.into();

        Substream {
            id,
            end,
            name,
            state,
            buffer: None,
        }
    }

    pub fn name(&self) -> Option<&Bytes> {
        self.name.as_ref()
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

// TODO: We always zero the buffer, we should delegate to the inner stream.
impl<T: AsyncRead, Buf: Array<Item = u8>> Read for Substream<T, Buf> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut lock = match self.state.poll_lock() {
            Async::Ready(lock) => lock,
            Async::NotReady => return Err(io::ErrorKind::WouldBlock.into()),
        };

        read_stream(&mut lock, (self.id, buf))
    }
}

impl<T: AsyncRead, Buf: Array<Item = u8>> AsyncRead for Substream<T, Buf> {}

impl<T: AsyncWrite, Buf: Array> Write for Substream<T, Buf> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut lock = match self.state.poll_lock() {
            Async::Ready(lock) => lock,
            Async::NotReady => return Err(io::ErrorKind::WouldBlock.into()),
        };

        let mut buffer = self.buffer
            .take()
            .unwrap_or_else(|| io::Cursor::new(buf_from_slice(buf)));

        let out = write_stream(
            &mut *lock,
            write::WriteRequest::substream(MultiplexHeader::message(self.id, self.end)),
            &mut buffer,
        );

        if buffer.position() < buffer.get_ref().len() as u64 {
            self.buffer = Some(buffer);
        }

        out
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut lock = match self.state.poll_lock() {
            Async::Ready(lock) => lock,
            Async::NotReady => return Err(io::ErrorKind::WouldBlock.into()),
        };

        lock.stream.flush()
    }
}

impl<T: AsyncWrite, Buf: Array> AsyncWrite for Substream<T, Buf> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        Ok(Async::Ready(()))
    }
}

pub struct InboundFuture<T, Buf: Array> {
    end: Endpoint,
    state: Arc<Mutex<MultiplexShared<T, Buf>>>,
}

impl<T: AsyncRead, Buf: Array<Item = u8>> Future for InboundFuture<T, Buf> {
    type Item = Substream<T, Buf>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut lock = match self.state.poll_lock() {
            Async::Ready(lock) => lock,
            Async::NotReady => return Ok(Async::NotReady),
        };

        // Attempt to make progress, but don't block if we can't
        match read_stream(&mut lock, None) {
            Ok(_) => {}
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {}
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

        lock.open_stream(id);

        Ok(Async::Ready(Substream::new(
            id,
            self.end,
            name,
            Arc::clone(&self.state),
        )))
    }
}

pub struct OutboundFuture<T, Buf: Array> {
    meta: Arc<MultiplexMetadata>,
    current_id: Option<(io::Cursor<ByteBuf>, u32)>,
    state: Arc<Mutex<MultiplexShared<T, Buf>>>,
}

impl<T, Buf: Array> OutboundFuture<T, Buf> {
    fn new(muxer: BufferedMultiplex<T, Buf>) -> Self {
        OutboundFuture {
            current_id: None,
            meta: muxer.meta,
            state: muxer.state,
        }
    }
}

fn nonce_to_id(id: usize, end: Endpoint) -> u32 {
    id as u32 * 2 + if end == Endpoint::Dialer { 0 } else { 1 }
}

impl<T: AsyncWrite, Buf: Array> Future for OutboundFuture<T, Buf> {
    type Item = Substream<T, Buf>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut lock = match self.state.poll_lock() {
            Async::Ready(lock) => lock,
            Async::NotReady => return Ok(Async::NotReady),
        };

        loop {
            let (mut id_str, id) = self.current_id.take().unwrap_or_else(|| {
                let next = nonce_to_id(
                    self.meta.nonce.fetch_add(1, atomic::Ordering::Relaxed),
                    self.meta.end,
                );
                (
                    io::Cursor::new(buf_from_slice(format!("{}", next).as_bytes())),
                    next as u32,
                )
            });

            match write_stream(
                &mut *lock,
                write::WriteRequest::meta(MultiplexHeader::open(id)),
                &mut id_str,
            ) {
                Ok(_) => {
                    debug_assert!(id_str.position() <= id_str.get_ref().len() as u64);
                    if id_str.position() == id_str.get_ref().len() as u64 {
                        if lock.open_stream(id) {
                            return Ok(Async::Ready(Substream::new(
                                id,
                                self.meta.end,
                                Bytes::from(&id_str.get_ref()[..]),
                                Arc::clone(&self.state),
                            )));
                        }
                    } else {
                        self.current_id = Some((id_str, id));
                        return Ok(Async::NotReady);
                    }
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    self.current_id = Some((id_str, id));

                    return Ok(Async::NotReady);
                }
                Err(other) => return Err(other),
            }
        }
    }
}

pub struct MultiplexMetadata {
    nonce: AtomicUsize,
    end: Endpoint,
}

pub type Multiplex<T> = BufferedMultiplex<T, [u8; 0]>;

pub struct BufferedMultiplex<T, Buf: Array> {
    meta: Arc<MultiplexMetadata>,
    state: Arc<Mutex<MultiplexShared<T, Buf>>>,
}

impl<T, Buf: Array> Clone for BufferedMultiplex<T, Buf> {
    fn clone(&self) -> Self {
        BufferedMultiplex {
            meta: self.meta.clone(),
            state: self.state.clone(),
        }
    }
}

impl<T, Buf: Array> BufferedMultiplex<T, Buf> {
    pub fn new(stream: T, end: Endpoint) -> Self {
        BufferedMultiplex {
            meta: Arc::new(MultiplexMetadata {
                nonce: AtomicUsize::new(0),
                end,
            }),
            state: Arc::new(Mutex::new(MultiplexShared::new(stream))),
        }
    }

    pub fn dial(stream: T) -> Self {
        Self::new(stream, Endpoint::Dialer)
    }

    pub fn listen(stream: T) -> Self {
        Self::new(stream, Endpoint::Listener)
    }
}

impl<T: AsyncRead + AsyncWrite, Buf: Array<Item = u8>> StreamMuxer for BufferedMultiplex<T, Buf> {
    type Substream = Substream<T, Buf>;
    type OutboundSubstream = OutboundFuture<T, Buf>;
    type InboundSubstream = InboundFuture<T, Buf>;

    fn inbound(self) -> Self::InboundSubstream {
        InboundFuture {
            state: Arc::clone(&self.state),
            end: self.meta.end,
        }
    }

    fn outbound(self) -> Self::OutboundSubstream {
        OutboundFuture::new(self)
    }
}

pub type MultiplexConfig = BufferedMultiplexConfig<[u8; 0]>;

#[derive(Debug, Copy, Clone)]
pub struct BufferedMultiplexConfig<Buf: Array>(std::marker::PhantomData<Buf>);

impl<Buf: Array> Default for BufferedMultiplexConfig<Buf> {
    fn default() -> Self {
        BufferedMultiplexConfig(std::marker::PhantomData)
    }
}

impl<Buf: Array> BufferedMultiplexConfig<Buf> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<C, Buf: Array> ConnectionUpgrade<C> for BufferedMultiplexConfig<Buf>
where
    C: AsyncRead + AsyncWrite,
{
    type Output = BufferedMultiplex<C, Buf>;
    type Future = FutureResult<BufferedMultiplex<C, Buf>, io::Error>;
    type UpgradeIdentifier = ();
    type NamesIter = iter::Once<(Bytes, ())>;

    #[inline]
    fn upgrade(self, i: C, _: (), end: Endpoint, _: &Multiaddr) -> Self::Future {
        future::ok(BufferedMultiplex::new(i, end))
    }

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/mplex/6.7.0"), ()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use header::PacketType;
    use std::io;
    use tokio_io::io as tokio;

    #[test]
    fn can_use_one_stream() {
        let message = b"Hello, world!";

        let stream = io::Cursor::new(Vec::new());

        let mplex = Multiplex::dial(stream);

        let mut substream = mplex.clone().outbound().wait().unwrap();

        assert!(tokio::write_all(&mut substream, message).wait().is_ok());

        let id = substream.id();

        assert_eq!(
            substream
                .name()
                .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()),
            Some(id.to_string())
        );

        let stream = io::Cursor::new(mplex.state.lock().wait().unwrap().stream.get_ref().clone());

        let mplex = Multiplex::listen(stream);

        let mut substream = mplex.inbound().wait().unwrap();

        assert_eq!(id, substream.id());
        assert_eq!(
            substream
                .name()
                .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()),
            Some(id.to_string())
        );

        let mut buf = vec![0; message.len()];

        assert!(tokio::read(&mut substream, &mut buf).wait().is_ok());
        assert_eq!(&buf, message);
    }

    #[test]
    fn can_use_many_streams() {
        let stream = io::Cursor::new(Vec::new());

        let mplex = Multiplex::dial(stream);

        let mut outbound: Vec<Substream<_>> = vec![];

        for _ in 0..5 {
            outbound.push(mplex.clone().outbound().wait().unwrap());
        }

        outbound.sort_by_key(|a| a.id());

        for (i, substream) in outbound.iter_mut().enumerate() {
            assert!(
                tokio::write_all(substream, i.to_string().as_bytes())
                    .wait()
                    .is_ok()
            );
        }

        let stream = io::Cursor::new(mplex.state.lock().wait().unwrap().stream.get_ref().clone());

        let mplex = Multiplex::listen(stream);

        let mut inbound: Vec<Substream<_>> = vec![];

        for _ in 0..5 {
            inbound.push(mplex.clone().inbound().wait().unwrap());
        }

        inbound.sort_by_key(|a| a.id());

        for (mut substream, outbound) in inbound.iter_mut().zip(outbound.iter()) {
            let id = outbound.id();
            assert_eq!(id, substream.id());
            assert_eq!(
                substream
                    .name()
                    .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()),
                Some(id.to_string())
            );

            let mut buf = [0; 3];
            assert_eq!(tokio::read(&mut substream, &mut buf).wait().unwrap().2, 1);
        }
    }

    #[test]
    fn packets_to_unopened_streams_are_dropped() {
        use std::iter;

        let message = b"Hello, world!";

        // We use a large dummy length to exercise ignoring data longer than `ignore_buffer.len()`
        let dummy_length = 1000;

        let input = iter::empty()
            // Open a stream
            .chain(varint::encode(MultiplexHeader::open(0).to_u64()))
            // 0-length body (stream has no name)
            .chain(varint::encode(0usize))

            // "Message"-type packet for an unopened stream
            .chain(
                varint::encode(
                    // ID for an unopened stream: 1
                    MultiplexHeader::message(1, Endpoint::Dialer).to_u64(),
                ).into_iter(),
            )
            // Body: `dummy_length` of zeroes
            .chain(varint::encode(dummy_length))
            .chain(iter::repeat(0).take(dummy_length))

            // "Message"-type packet for an opened stream
            .chain(
                varint::encode(
                    // ID for an opened stream: 0
                    MultiplexHeader::message(0, Endpoint::Dialer).to_u64(),
                ).into_iter(),
            )
            .chain(varint::encode(message.len()))
            .chain(message.iter().cloned())

            .collect::<Vec<_>>();

        let mplex = Multiplex::listen(io::Cursor::new(input));

        let mut substream = mplex.inbound().wait().unwrap();

        assert_eq!(substream.id(), 0);
        assert_eq!(substream.name(), None);

        let mut buf = vec![0; message.len()];

        assert!(tokio::read(&mut substream, &mut buf).wait().is_ok());
        assert_eq!(&buf, message);
    }

    #[test]
    fn can_close_streams() {
        use std::iter;

        // Dummy data in the body of the close packet (since the de facto protocol is to accept but
        // ignore this data)
        let dummy_length = 64;

        let input = iter::empty()
            // Open a stream
            .chain(varint::encode(MultiplexHeader::open(0).to_u64()))
            // 0-length body (stream has no name)
            .chain(varint::encode(0usize))

            // Immediately close the stream
            .chain(
                varint::encode(
                    // ID for an unopened stream: 1
                    MultiplexHeader {
                        packet_type: PacketType::Close(Endpoint::Dialer),
                        substream_id: 0,
                    }.to_u64(),
                ).into_iter(),
            )
            .chain(varint::encode(dummy_length))
            .chain(iter::repeat(0).take(dummy_length))

            // Send packet to the closed stream
            .chain(
                varint::encode(
                    // ID for an opened stream: 0
                    MultiplexHeader::message(0, Endpoint::Dialer).to_u64(),
                ).into_iter(),
            )
            .chain(varint::encode(dummy_length))
            .chain(iter::repeat(0).take(dummy_length))

            .collect::<Vec<_>>();

        let mplex = Multiplex::listen(io::Cursor::new(input));

        let mut substream = mplex.inbound().wait().unwrap();

        assert_eq!(substream.id(), 0);
        assert_eq!(substream.name(), None);

        assert_eq!(
            tokio::read(&mut substream, &mut [0; 100][..])
                .wait()
                .unwrap()
                .2,
            0
        );
    }

    #[test]
    fn real_world_data() {
        let data: Vec<u8> = vec![
            // Open stream 1
            8,
            0,

            // Message for stream 1 (length 20)
            10,
            20,
            19,
            47,
            109,
            117,
            108,
            116,
            105,
            115,
            116,
            114,
            101,
            97,
            109,
            47,
            49,
            46,
            48,
            46,
            48,
            10,
        ];

        let mplex = Multiplex::listen(io::Cursor::new(data));

        let mut substream = mplex.inbound().wait().unwrap();

        assert_eq!(substream.id(), 1);

        assert_eq!(substream.name(), None);

        let mut out = vec![];

        for _ in 0..20 {
            let mut buf = [0; 1];

            assert_eq!(
                tokio::read(&mut substream, &mut buf[..]).wait().unwrap().2,
                1
            );

            out.push(buf[0]);
        }

        assert_eq!(out[0], 19);
        assert_eq!(&out[1..0x14 - 1], b"/multistream/1.0.0");
        assert_eq!(out[0x14 - 1], 0x0a);
    }

    #[test]
    fn can_buffer() {
        type Buffer = [u8; 1024];

        let stream = io::Cursor::new(Vec::new());

        let mplex = BufferedMultiplex::<_, Buffer>::dial(stream);

        let mut outbound: Vec<Substream<_, Buffer>> = vec![];

        for _ in 0..5 {
            outbound.push(mplex.clone().outbound().wait().unwrap());
        }

        outbound.sort_by_key(|a| a.id());

        for (i, substream) in outbound.iter_mut().enumerate() {
            assert!(
                tokio::write_all(substream, i.to_string().as_bytes())
                    .wait()
                    .is_ok()
            );
        }

        let stream = io::Cursor::new(mplex.state.lock().wait().unwrap().stream.get_ref().clone());

        let mplex = BufferedMultiplex::<_, Buffer>::listen(stream);

        let mut inbound: Vec<Substream<_, Buffer>> = vec![];

        for _ in 0..5 {
            let inb: Substream<_, Buffer> = mplex.clone().inbound().wait().unwrap();
            inbound.push(inb);
        }

        inbound.sort_by_key(|a| a.id());

        // Skip the first substream and let it be cached.
        for (mut substream, outbound) in inbound.iter_mut().zip(outbound.iter()).skip(1) {
            let id = outbound.id();
            assert_eq!(id, substream.id());
            assert_eq!(
                substream
                    .name()
                    .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()),
                Some(id.to_string())
            );

            let mut buf = [0; 3];
            assert_eq!(tokio::read(&mut substream, &mut buf).wait().unwrap().2, 1);
        }

        let (mut substream, outbound) = (&mut inbound[0], &outbound[0]);
        let id = outbound.id();
        assert_eq!(id, substream.id());
        assert_eq!(
            substream
                .name()
                .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()),
            Some(id.to_string())
        );

        let mut buf = [0; 3];
        assert_eq!(tokio::read(&mut substream, &mut buf).wait().unwrap().2, 1);
    }
}
