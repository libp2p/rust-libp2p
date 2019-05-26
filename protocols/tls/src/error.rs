//! Defines the `TlsError` enum that groups all possible errors in TLS.

use std::error;
use std::io::Error as IoError;
use std::fmt;

#[derive(Debug)]
pub enum TlsError {
    IoError(IoError)
}

impl error::Error for TlsError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            TlsError::IoError(ref err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for TlsError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            TlsError::IoError(e) =>
                write!(f, "I/O error: {}", e),
        }
    }
}
