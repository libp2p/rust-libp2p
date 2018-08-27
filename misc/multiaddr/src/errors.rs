use std::{net, fmt, error, io, num, str, string};
use bs58;
use multihash;
use byteorder;
use unsigned_varint::decode;

pub type Result<T> = ::std::result::Result<T, Error>;

/// Error types
#[derive(Debug)]
pub enum Error {
    UnknownProtocol,
    UnknownProtocolString,
    InvalidMultiaddr,
    MissingAddress,
    ParsingError(Box<error::Error + Send + Sync>),
    InvalidUvar(decode::Error)
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::UnknownProtocol => f.write_str("unknown protocol"),
            Error::UnknownProtocolString => f.write_str("unknown protocol string"),
            Error::InvalidMultiaddr => f.write_str("invalid multiaddr"),
            Error::MissingAddress => f.write_str("protocol requires address, none given"),
            Error::ParsingError(e) => write!(f, "failed to parse: {}", e),
            Error::InvalidUvar(e) => write!(f, "failed to decode unsigned varint: {}", e)
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

impl From<bs58::decode::DecodeError> for Error {
    fn from(err: bs58::decode::DecodeError) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<net::AddrParseError> for Error {
    fn from(err: net::AddrParseError) -> Error {
        Error::ParsingError(err.into())
    }
}

impl From<byteorder::Error> for Error {
    fn from(err: byteorder::Error) -> Error {
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

