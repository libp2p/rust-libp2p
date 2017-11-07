use multiaddr::Multiaddr;
use futures::{IntoFuture, Future};
use futures::stream::Stream;
use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};

/// A transport is a stream producing incoming connections.
/// These are transports or wrappers around them.
pub trait Transport {
    /// The raw connection.
    type RawConn: AsyncRead + AsyncWrite;

    /// The listener produces incoming connections.
    type Listener: Stream<Item = Self::RawConn>;

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
