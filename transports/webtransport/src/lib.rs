mod certificate;
mod config;
mod connection;
mod transport;

use wtransport::error::ConnectionError;

pub use certificate::{CertHash, Certificate};
pub use config::Config;
pub(crate) use connection::{Connecting, Connection, Stream};
use libp2p_core::transport::TransportError;
pub use transport::GenTransport;

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

    #[error(transparent)]
    AuthenticationError(#[from] libp2p_noise::Error),

    #[error("Unknown remote peer ID")]
    UnknownRemotePeerId,

    /// The [`Connecting`] future timed out.
    #[error("Handshake with the remote timed out.")]
    HandshakeTimedOut,
}

impl From<Error> for TransportError<Error> {
    fn from(value: Error) -> Self {
        TransportError::Other(value)
    }
}
