use failure::Fail;
use openssl::error::ErrorStack;
use picoquic;
use std::{error::Error, fmt, io};

#[derive(Debug)]
pub struct QuicError {
    kind: ErrorKind,
    source: Option<Box<dyn Error + Send + Sync>>
}

#[derive(Debug)]
pub enum ErrorKind {
    Io,
    PicoQuic,
    OpenSsl,
    InvalidPeerId,
    PeerIdMismatch,
    #[doc(hidden)]
    __Nonexhaustive
}

impl Into<QuicError> for ErrorKind {
    fn into(self) -> QuicError {
        QuicError { kind: self, source: None }
    }
}

impl fmt::Display for QuicError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            ErrorKind::Io => write!(f, "i/o: {:?}", self.source),
            ErrorKind::PicoQuic => write!(f, "picoquic: {:?}", self.source),
            ErrorKind::OpenSsl => write!(f, "openssl: {:?}", self.source),
            ErrorKind::InvalidPeerId => f.write_str("invalid peer ID"),
            ErrorKind::PeerIdMismatch => f.write_str("peer IDs do not match"),
            ErrorKind::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl Error for QuicError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Some(ref s) = self.source {
            Some(s.as_ref())
        } else {
            None
        }
    }
}

impl From<io::Error> for QuicError {
    fn from(e: io::Error) -> Self {
        QuicError {
            kind: ErrorKind::Io,
            source: Some(Box::new(e))
        }
    }
}

impl From<ErrorStack> for QuicError {
    fn from(e: ErrorStack) -> Self {
        QuicError {
            kind: ErrorKind::OpenSsl,
            source: Some(Box::new(e))
        }
    }
}

impl From<picoquic::Error> for QuicError {
    fn from(e: picoquic::Error) -> Self {
        QuicError {
            kind: ErrorKind::PicoQuic,
            source: Some(Box::new(e.compat()))
        }
    }
}

