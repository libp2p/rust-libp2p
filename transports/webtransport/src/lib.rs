mod transport;
mod connection;
mod config;
mod certificate;

use wtransport::error::ConnectionError;

pub use transport::GenTransport;
pub use connection::{Connecting, Connection};
pub use certificate::CertHash;
use libp2p_core::transport::TransportError;
// pub(crate) use connection::Stream;

/// Errors that may happen on the [`GenTransport`] or a single [`Connection`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error after the remote has been reached.
    #[error(transparent)]
    Connection(#[from] ConnectionError),

    /// I/O Error on a socket.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The [`Connecting`] future timed out.
    #[error("Unexpected HTTP endpoint of a libp2p WebTransport server {0}")]
    UnexpectedPath(String),

    /// The [`Connecting`] future timed out.
    #[error("Handshake with the remote timed out.")]
    HandshakeTimedOut,
}

impl From<Error> for TransportError<Error> {
    fn from(value: Error) -> Self {
        TransportError::Other(value)
    }
}