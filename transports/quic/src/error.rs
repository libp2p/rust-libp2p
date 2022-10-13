// Copyright 2022 Protocol Labs.
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

/// Error that can happen on the transport.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error while trying to reach a remote.
    #[error(transparent)]
    Reach(#[from] ConnectError),

    /// Error after the remote has been reached.
    #[error(transparent)]
    Connection(#[from] ConnectionError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The task driving the endpoint has crashed.
    #[error("Endpoint driver crashed")]
    EndpointDriverCrashed,

    #[error("Handshake with the remote timed out.")]
    HandshakeTimedOut,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ConnectError(#[from] quinn_proto::ConnectError);

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ConnectionError(#[from] quinn_proto::ConnectionError);
