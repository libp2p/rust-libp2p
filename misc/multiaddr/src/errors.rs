use std::{net, fmt, error, io, num, str, string};
use bs58;
use multihash;
use unsigned_varint::decode;

pub type Result<T> = ::std::result::Result<T, Error>;

/// Error types
#[derive(Debug)]
pub enum Error {
    DataLessThanLen,
    InvalidMultiaddr,
    InvalidProtocolString,
    InvalidUvar(decode::Error),
    ParsingError(Box<dyn error::Error + Send + Sync>),
    UnknownProtocolId(u32),
    UnknownProtocolString,
    #[doc(hidden)]
    __Nonexhaustive
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::DataLessThanLen => f.write_str("we have less data than indicated by length"),
            Error::InvalidMultiaddr => f.write_str("invalid multiaddr"),
            Error::InvalidProtocolString => f.write_str("invalid protocol string"),
            Error::InvalidUvar(e) => write!(f, "failed to decode unsigned varint: {}", e),
            Error::ParsingError(e) => write!(f, "failed to parse: {}", e),
            Error::UnknownProtocolId(id) => write!(f, "unknown protocol id: {}", id),
            Error::UnknownProtocolString => f.write_str("unknown protocol string"),
            Error::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl error::Error for Error {
    #[inline]
    fn cause(&self) -> Option<&dyn error::Error> {
        if let Error::ParsingError(e) = self {
            Some(&**e)
        } else {
            None
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<multihash::DecodeOwnedError> for Error {
    fn from(err: multihash::DecodeOwnedError) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<bs58::decode::Error> for Error {
    fn from(err: bs58::decode::Error) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<net::AddrParseError> for Error {
    fn from(err: net::AddrParseError) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<num::ParseIntError> for Error {
    fn from(err: num::ParseIntError) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<string::FromUtf8Error> for Error {
    fn from(err: string::FromUtf8Error) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<str::Utf8Error> for Error {
    fn from(err: str::Utf8Error) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<decode::Error> for Error {
    fn from(e: decode::Error) -> Error {
        Error::InvalidUvar(e)
    }
}

