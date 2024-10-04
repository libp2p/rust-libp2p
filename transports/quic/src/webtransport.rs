use std::fmt::{Debug, Display, Formatter};
use std::io;
use std::io::ErrorKind;

use h3::ext::Protocol;
use http::Method;

pub(crate) use certificates::alpn_protocols;
pub(crate) use certificates::Certificate;
pub(crate) use connection::accept_webtransport_stream;
pub(crate) use connection::Connection;

use crate::Error;

mod certificates;
mod connection;

pub(crate) const WEBTRANSPORT_PATH: &str = "/.well-known/libp2p-webtransport";
pub(crate) const NOISE_QUERY: &str = "type=noise";

#[derive(Debug)]
pub enum WebtransportConnectingError {
    UnexpectedProtocol(Option<Protocol>),
    UnexpectedMethod(Method),
    UnexpectedPath(String),
    UnexpectedQuery(String),

    ConnectionError(quinn::ConnectionError),
    Http3Error(h3::Error),
    NoiseError(libp2p_noise::Error),
    NoMoreStreams,
    CrateError(Error),
}

impl Display for WebtransportConnectingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            WebtransportConnectingError::UnexpectedProtocol(p) => {
                write!(f, "Unexpected request protocol {:?}", p)
            }
            WebtransportConnectingError::UnexpectedMethod(m) => {
                write!(f, "Unexpected initial request method {:?}", m)
            }
            WebtransportConnectingError::UnexpectedPath(p) => {
                write!(f, "Unexpected initial request path {}", p)
            }
            WebtransportConnectingError::UnexpectedQuery(q) => {
                write!(f, "Unexpected initial request query {}", q)
            }
            WebtransportConnectingError::ConnectionError(e) => write!(f, "Connection error: {}", e),
            WebtransportConnectingError::Http3Error(e) => write!(f, "Http3 error: {}", e),
            WebtransportConnectingError::NoiseError(e) => write!(f, "Noise error: {}", e),
            WebtransportConnectingError::NoMoreStreams => write!(f, "No more streams"),
            WebtransportConnectingError::CrateError(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for WebtransportConnectingError {}

impl From<Error> for WebtransportConnectingError {
    fn from(value: Error) -> Self {
        WebtransportConnectingError::CrateError(value)
    }
}

impl From<WebtransportConnectingError> for Error {
    fn from(value: WebtransportConnectingError) -> Self {
        if let WebtransportConnectingError::CrateError(e) = value {
            e
        } else {
            Error::Io(io::Error::new(ErrorKind::Other, value))
        }
    }
}

impl From<libp2p_noise::Error> for WebtransportConnectingError {
    fn from(e: libp2p_noise::Error) -> Self {
        WebtransportConnectingError::NoiseError(e)
    }
}

impl From<h3::Error> for WebtransportConnectingError {
    fn from(e: h3::Error) -> Self {
        WebtransportConnectingError::Http3Error(e)
    }
}

impl From<quinn::ConnectionError> for WebtransportConnectingError {
    fn from(e: quinn::ConnectionError) -> Self {
        WebtransportConnectingError::ConnectionError(e)
    }
}
