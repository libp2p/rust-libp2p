use std::io::Error as IoError;
use futures::IntoFuture;
use tokio_io::{AsyncRead, AsyncWrite};

// Something more strongly-typed?
pub type ProtocolId = String;
pub type PeerId = String;

/// A logical wire between us and a peer. We can read and write through this asynchronously.
///
/// You can have multiple `Socket`s between you and any given peer.
pub trait Socket: AsyncRead + AsyncWrite + Sized {
    type Conn: Conn<Socket = Self>;

    /// Get the protocol ID this socket uses.
    fn protocol_id(&self) -> ProtocolId;

    /// Access the underlying connection.
    fn conn(&self) -> &Self::Conn;
}

/// A connection between you and a peer.
pub trait Conn {
    /// The socket type this connection manages.
    type Socket;
    type SocketFuture: IntoFuture<Item = Self::Socket, Error = IoError>;

    /// Initiate a socket between you and the peer on the given protocol.
    fn make_socket(&self, proto: ProtocolId) -> Self::SocketFuture;
}
