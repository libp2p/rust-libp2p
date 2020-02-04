// Copyright 2020 Parity Technologies (UK) Ltd.
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

use err_derive::Error;
use io::ErrorKind;
use ring::error::Unspecified;
use std::io;
#[derive(Error, Debug)]
/// An error that can be returned by libp2p-quic.
pub enum Error {
    #[error(display = "Fatal I/O error {}", _0)]
    IO(#[error(source)] std::io::Error),
    #[error(display = "Peer sent a malformed certificate")]
    BadCertificate(#[error(source)] ring::error::Unspecified),
    #[error(display = "QUIC protocol error: {}", _0)]
    ConnectionError(#[error(source)] quinn_proto::ConnectionError),
    #[error(display = "Cannot establish connection: {}", _0)]
    CannotConnect(#[error(source)] quinn_proto::ConnectError),
    #[error(display = "Peer stopped receiving data: code {}", _0)]
    Stopped(quinn_proto::VarInt),
    #[error(display = "Connection was prematurely closed")]
    ConnectionLost,
    #[error(display = "Cannot listen on the same endpoint more than once")]
    AlreadyListening,
    /// The stream was reset by the peer.
    #[error(display = "Peer reset stream: code {}", _0)]
    Reset(quinn_proto::VarInt),
    /// Either an attempt was made to write to a stream that was already shut down,
    /// or a previous operation on this stream failed.
    #[error(display = "Use of a stream that has is no longer valid. This is a \
                       bug in the application.")]
    ExpiredStream,
    #[error(display = "Trying to ready from a stream that the peer has not been \
    notified of.  For performance and complexity reasons, libp2p-quic does not \
    notify the peer that a stream is opened until at least one byte is sent.  \
    Therefore, this read would deadlock.")]
    CannotReadFromUnwrittenStream,
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::IO(e) => io::Error::new(e.kind(), Error::IO(e)),
            e @ Error::BadCertificate(Unspecified) => io::Error::new(ErrorKind::InvalidData, e),
            Error::ConnectionError(e) => e.into(),
            e @ Error::CannotConnect(_) => io::Error::new(ErrorKind::Other, e),
            e @ Error::Stopped(_) | e @ Error::Reset(_) | e @ Error::ConnectionLost => {
                io::Error::new(ErrorKind::ConnectionAborted, e)
            }
            e @ Error::ExpiredStream => io::Error::new(ErrorKind::BrokenPipe, e),
            e @ Error::AlreadyListening => io::Error::new(ErrorKind::AddrInUse, e),
            e @ Error::CannotReadFromUnwrittenStream => io::Error::new(ErrorKind::NotConnected, e),
        }
    }
}
