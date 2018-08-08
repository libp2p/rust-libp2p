use std::{fmt, error};

/// Error that can happen when encoding some bytes into a multihash.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// The requested hash algorithm isn't supported by this library.
    UnsupportedType,
}

impl fmt::Display for EncodeError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(error::Error::description(self))
    }
}

impl error::Error for EncodeError {
    #[inline]
    fn description(&self) -> &str {
        match *self {
            EncodeError::UnsupportedType => "This type is not supported yet",
        }
    }
}

/// Error that can happen when decoding some bytes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The input doesn't have a correct length.
    BadInputLength,
    /// The code of the hashing algorithm is incorrect.
    UnknownCode,
}

impl fmt::Display for DecodeError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(error::Error::description(self))
    }
}

impl error::Error for DecodeError {
    #[inline]
    fn description(&self) -> &str {
        match *self {
            DecodeError::BadInputLength => "Not matching input length",
            DecodeError::UnknownCode => "Found unknown code",
        }
    }
}

/// Error that can happen when decoding some bytes.
///
/// Same as `DecodeError`, but allows retreiving the data whose decoding was attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeOwnedError {
    /// The error.
    pub error: DecodeError,
    /// The data whose decoding was attempted.
    pub data: Vec<u8>,
}

impl fmt::Display for DecodeOwnedError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(error::Error::description(self))
    }
}

impl error::Error for DecodeOwnedError {
    #[inline]
    fn description(&self) -> &str {
        self.error.description()
    }
}
