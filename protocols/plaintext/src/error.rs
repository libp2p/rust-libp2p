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
use protobuf::error::ProtobufError;

#[derive(Debug)]
pub enum PlainTextError {
    /// I/O error.
    IoError(IoError),

    /// Protocol buffer error.
    ProtobufError(ProtobufError),

    /// Failed to parse one of the handshake protobuf messages.
    HandshakeParsingFailure,
}

impl error::Error for PlainTextError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            PlainTextError::IoError(ref err) => Some(err),
            PlainTextError::ProtobufError(ref err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for PlainTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            PlainTextError::IoError(e) => write!(f, "I/O error: {}", e),
            PlainTextError::ProtobufError(e) => write!(f, "Protobuf error: {}", e),
            PlainTextError::HandshakeParsingFailure => f.write_str("Failed to parse one of the handshake protobuf messages"),
        }
    }
}

impl From<IoError> for PlainTextError {
    #[inline]
    fn from(err: IoError) -> PlainTextError {
        PlainTextError::IoError(err)
    }
}

impl From<ProtobufError> for PlainTextError {
    #[inline]
    fn from(err: ProtobufError) -> PlainTextError {
        PlainTextError::ProtobufError(err)
    }
}
