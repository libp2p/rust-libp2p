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

use std::error;
use std::fmt;
use std::io::Error as IoError;

#[derive(Debug)]
pub enum PlainTextError {
    /// I/O error.
    IoError(IoError),

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
pub struct DecodeError(pub(crate) quick_protobuf::Error);

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

impl error::Error for PlainTextError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            PlainTextError::IoError(ref err) => Some(err),
            PlainTextError::InvalidPayload(ref err) => Some(err),
            PlainTextError::InvalidPublicKey(ref err) => Some(err),
            PlainTextError::InvalidPeerId(ref err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for PlainTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            PlainTextError::IoError(e) => write!(f, "I/O error: {e}"),
            PlainTextError::InvalidPayload(_) => f.write_str("Failed to decode protobuf"),
            PlainTextError::PeerIdMismatch => f.write_str(
                "The peer id of the exchange isn't consistent with the remote public key",
            ),
            PlainTextError::InvalidPublicKey(_) => f.write_str("Failed to decode public key"),
            PlainTextError::InvalidPeerId(_) => f.write_str("Failed to decode PeerId"),
        }
    }
}

impl From<IoError> for PlainTextError {
    fn from(err: IoError) -> PlainTextError {
        PlainTextError::IoError(err)
    }
}

impl From<DecodeError> for PlainTextError {
    fn from(err: DecodeError) -> PlainTextError {
        PlainTextError::InvalidPayload(err)
    }
}

impl From<libp2p_identity::DecodingError> for PlainTextError {
    fn from(err: libp2p_identity::DecodingError) -> PlainTextError {
        PlainTextError::InvalidPublicKey(err)
    }
}

impl From<libp2p_identity::ParseError> for PlainTextError {
    fn from(err: libp2p_identity::ParseError) -> PlainTextError {
        PlainTextError::InvalidPeerId(err)
    }
}
