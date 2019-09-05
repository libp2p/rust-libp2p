use std::{error, fmt};

/// Error that can happen when encoding some bytes into a multihash.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// The requested hash algorithm isn't supported by this library.
    UnsupportedType,
    /// The input length is too large for the hash algorithm.
    UnsupportedInputLength,
}

impl fmt::Display for EncodeError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            EncodeError::UnsupportedType => write!(f, "This type is not supported yet"),
            EncodeError::UnsupportedInputLength => write!(
                f,
                "The length of the input for the given hash is not yet supported"
            ),
        }
    }
}

impl error::Error for EncodeError {}

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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            DecodeError::BadInputLength => write!(f, "Not matching input length"),
            DecodeError::UnknownCode => write!(f, "Found unknown code"),
        }
    }
}

impl error::Error for DecodeError {}

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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl error::Error for DecodeOwnedError {}
