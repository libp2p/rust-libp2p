use std::{fmt, error};

/// Error that can happen when encoding some bytes into a multihash.
#[derive(Debug)]
pub enum EncodeError {
    /// The requested hash algorithm isn't supported by this library.
    UnsupportedType,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(error::Error::description(self))
    }
}

impl error::Error for EncodeError {
    fn description(&self) -> &str {
        match *self {
            EncodeError::UnsupportedType => "This type is not supported yet",
        }
    }
}

/// Error that can happen when decoding some bytes.
#[derive(Debug)]
pub enum DecodeError {
    /// The input doesn't have a correct length.
    BadInputLength,
    /// The code of the hashing algorithm is incorrect.
    UnknownCode,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(error::Error::description(self))
    }
}

impl error::Error for DecodeError {
    fn description(&self) -> &str {
        match *self {
            DecodeError::BadInputLength => "Not matching input length",
            DecodeError::UnknownCode => "Found unknown code",
        }
    }
}
