// Copyright 2017 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! Contains the error structs for the low-level protocol handling.

use std::error::Error;
use std::fmt;
use std::io;
use unsigned_varint::decode;

/// Error at the multistream-select layer of communication.
#[derive(Debug)]
pub enum MultistreamSelectError {
    /// I/O error.
    IoError(io::Error),

    /// The remote doesn't use the same multistream-select protocol as we do.
    FailedHandshake,

    /// Received an unknown message from the remote.
    UnknownMessage,

    /// Protocol names must always start with `/`, otherwise this error is returned.
    InvalidProtocolName,

    /// Too many protocols have been returned by the remote.
    TooManyProtocols,
}

impl From<io::Error> for MultistreamSelectError {
    fn from(err: io::Error) -> MultistreamSelectError {
        MultistreamSelectError::IoError(err)
    }
}

impl From<decode::Error> for MultistreamSelectError {
    fn from(err: decode::Error) -> MultistreamSelectError {
        Self::from(io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
    }
}

impl Error for MultistreamSelectError {
    fn description(&self) -> &str {
        match *self {
            MultistreamSelectError::IoError(_) => "I/O error",
            MultistreamSelectError::FailedHandshake => {
                "the remote doesn't use the same multistream-select protocol as we do"
            }
            MultistreamSelectError::UnknownMessage => "received an unknown message from the remote",
            MultistreamSelectError::InvalidProtocolName => {
                "protocol names must always start with `/`, otherwise this error is returned"
            }
            MultistreamSelectError::TooManyProtocols =>
                "Too many protocols."
        }
    }

    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            MultistreamSelectError::IoError(ref err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for MultistreamSelectError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(fmt, "{}", Error::description(self))
    }
}
