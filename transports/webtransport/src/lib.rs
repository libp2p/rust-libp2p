mod certificate;
mod config;
mod connection;
mod transport;

pub use self::{
    certificate::{CertHash, Certificate},
    config::Config,
    connection::{Connection, Stream},
    transport::GenTransport,
};
pub(crate) use connection::Connecting;
use libp2p_core::transport::TransportError;
use wtransport::error::ConnectionError;

/// Errors that may happen on the [`GenTransport`] or a single [`Connection`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error after the remote has been reached.
    #[error(transparent)]
    Connection(#[from] ConnectionError),

    /// I/O Error on a socket.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Unexpected HTTP endpoint of a libp2p WebTransport server {0}")]
    UnexpectedPath(String),

    #[error(transparent)]
    AuthenticationError(#[from] libp2p_noise::Error),

    #[error("Unknown remote peer ID")]
    UnknownRemotePeerId,

    #[error("Handshake with the remote timed out")]
    HandshakeTimedOut,

    #[error("Dial operation is not allowed on a libp2p WebTransport server")]
    DialOperationIsNotAllowed,
}

impl From<Error> for TransportError<Error> {
    fn from(value: Error) -> Self {
        TransportError::Other(value)
    }
}
