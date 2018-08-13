use std::{net, fmt, error, io, num, string};
use bs58;
use multihash;
use byteorder;

pub type Result<T> = ::std::result::Result<T, Error>;

/// Error types
#[derive(Debug)]
pub enum Error {
    UnknownProtocol,
    UnknownProtocolString,
    InvalidMultiaddr,
    MissingAddress,
    ParsingError(Box<error::Error + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(error::Error::description(self))
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::UnknownProtocol => "unknown protocol",
            Error::UnknownProtocolString => "unknown protocol string",
            Error::InvalidMultiaddr => "invalid multiaddr",
            Error::MissingAddress => "protocol requires address, none given",
            Error::ParsingError(_) => "failed to parse",
        }
    }

    #[inline]
    fn cause(&self) -> Option<&error::Error> {
        match *self {
            Error::ParsingError(ref err) => Some(&**err),
            _ => None
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
