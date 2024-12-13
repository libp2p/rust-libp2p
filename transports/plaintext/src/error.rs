// Copyright 2019 Parity Technologies (UK) Ltd.
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

use std::{error, fmt, io::Error as IoError};

#[derive(Debug)]
pub enum Error {
    /// I/O error.
    Io(IoError),

    /// Failed to parse the handshake protobuf message.
    InvalidPayload(DecodeError),

    /// Failed to parse public key from bytes in protobuf message.
    InvalidPublicKey(libp2p_identity::DecodingError),

    /// Failed to parse the [`PeerId`](libp2p_identity::PeerId) from bytes in the protobuf message.
    InvalidPeerId(libp2p_identity::ParseError),

    /// The peer id of the exchange isn't consistent with the remote public key.
    PeerIdMismatch,
}

#[derive(Debug)]
pub struct DecodeError(pub(crate) quick_protobuf_codec::Error);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.0.source()
    }
}

impl error::Error for Error {
    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            Error::Io(ref err) => Some(err),
            Error::InvalidPayload(ref err) => Some(err),
            Error::InvalidPublicKey(ref err) => Some(err),
            Error::InvalidPeerId(ref err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::InvalidPayload(_) => f.write_str("Failed to decode protobuf"),
            Error::PeerIdMismatch => f.write_str(
                "The peer id of the exchange isn't consistent with the remote public key",
            ),
            Error::InvalidPublicKey(_) => f.write_str("Failed to decode public key"),
            Error::InvalidPeerId(_) => f.write_str("Failed to decode PeerId"),
        }
    }
}

impl From<IoError> for Error {
    fn from(err: IoError) -> Error {
        Error::Io(err)
    }
}

impl From<DecodeError> for Error {
    fn from(err: DecodeError) -> Error {
        Error::InvalidPayload(err)
    }
}

impl From<libp2p_identity::DecodingError> for Error {
    fn from(err: libp2p_identity::DecodingError) -> Error {
        Error::InvalidPublicKey(err)
    }
}

impl From<libp2p_identity::ParseError> for Error {
    fn from(err: libp2p_identity::ParseError) -> Error {
        Error::InvalidPeerId(err)
    }
}
