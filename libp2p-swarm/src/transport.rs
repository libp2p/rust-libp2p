use multiaddr::Multiaddr;
use futures::{IntoFuture, Future, Poll, Async};
use futures::stream::Stream;
use std::io::{Cursor, Read, Write, Error as IoError};
use tokio_io::{AsyncRead, AsyncWrite};

/// A transport is a stream producing incoming connections.
/// These are transports or wrappers around them.
pub trait Transport {
    /// The raw connection.
    type RawConn: AsyncRead + AsyncWrite;

    /// The listener produces incoming connections.
    type Listener: Stream<Item = Self::RawConn, Error = IoError>;

    /// A future which indicates currently dialing to a peer.
    type Dial: IntoFuture<Item = Self::RawConn, Error = IoError>;

    /// Listen on the given multi-addr.
    /// Returns the address back if it isn't supported.
    fn listen_on(&mut self, addr: Multiaddr) -> Result<Self::Listener, Multiaddr>;

    /// Dial to the given multi-addr.
    /// Returns either a future which may resolve to a connection,
    /// or gives back the multiaddress.
    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, Multiaddr>;
}

// TODO: could use `!` for associated types
impl Transport for () {
    type RawConn = Cursor<Vec<u8>>;
    type Listener = Box<Stream<Item = Self::RawConn, Error = IoError>>;
    type Dial = Box<Future<Item = Self::RawConn, Error = IoError>>;

    #[inline]
    fn listen_on(&mut self, addr: Multiaddr) -> Result<Self::Listener, Multiaddr> {
        Err(addr)
    }

    #[inline]
    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, Multiaddr> {
        Err(addr)
    }
}

impl<A, B> Transport for (A, B)
    where A: Transport,
          B: Transport,
{
    type RawConn = Either<A::RawConn, B::RawConn>;
    type Listener = Either<A::Listener, B::Listener>;
    type Dial = Either<<A::Dial as IntoFuture>::Future, <B::Dial as IntoFuture>::Future>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<Self::Listener, Multiaddr> {
        let addr = match self.0.listen_on(addr) {
            Ok(connec) => return Ok(Either::First(connec)),
            Err(addr) => addr,
        };

        self.1.listen_on(addr).map(|s| Either::Second(s))
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, Multiaddr> {
        let addr = match self.0.dial(addr) {
            Ok(connec) => return Ok(Either::First(connec.into_future())),
            Err(addr) => addr,
        };

        self.1.dial(addr).map(|s| Either::Second(s.into_future()))
    }
}

pub enum Either<A, B> {
    First(A),
    Second(B),
}

impl<A, B> Stream for Either<A, B>
    where A: Stream<Error = IoError>,
          B: Stream<Error = IoError>,
{
    type Item = Either<A::Item, B::Item>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self {
            &mut Either::First(ref mut a) => a.poll().map(|i| i.map(|v| v.map(Either::First))),
            &mut Either::Second(ref mut a) => a.poll().map(|i| i.map(|v| v.map(Either::Second))),
        }
    }
}

impl<A, B> Future for Either<A, B>
    where A: Future<Error = IoError>,
          B: Future<Error = IoError>,
{
    type Item = Either<A::Item, B::Item>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self {
            &mut Either::First(ref mut a) => {
                let item = try_ready!(a.poll());
                Ok(Async::Ready(Either::First(item)))
            },
            &mut Either::Second(ref mut b) => {
                let item = try_ready!(b.poll());
                Ok(Async::Ready(Either::Second(item)))
            },
        }
    }
}

impl<A, B> AsyncRead for Either<A, B>
    where A: AsyncRead,
          B: AsyncRead,
{
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match self {
            &Either::First(ref a) => a.prepare_uninitialized_buffer(buf),
            &Either::Second(ref b) => b.prepare_uninitialized_buffer(buf),
        }
    }
}

impl<A, B> Read for Either<A, B>
    where A: Read,
          B: Read,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        match self {
            &mut Either::First(ref mut a) => a.read(buf),
            &mut Either::Second(ref mut b) => b.read(buf),
        }
    }
}

impl<A, B> AsyncWrite for Either<A, B>
    where A: AsyncWrite,
          B: AsyncWrite,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        match self {
            &mut Either::First(ref mut a) => a.shutdown(),
            &mut Either::Second(ref mut b) => b.shutdown(),
        }
    }
}

impl<A, B> Write for Either<A, B>
    where A: Write,
          B: Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        match self {
            &mut Either::First(ref mut a) => a.write(buf),
            &mut Either::Second(ref mut b) => b.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        match self {
            &mut Either::First(ref mut a) => a.flush(),
            &mut Either::Second(ref mut b) => b.flush(),
        }
    }
}
