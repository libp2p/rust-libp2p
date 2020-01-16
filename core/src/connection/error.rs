// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::transport::TransportError;
use std::{io, fmt};

/// Errors that can occur in the context of a pending or established `Connection`.
#[derive(Debug)]
pub enum ConnectionError<THandlerErr, TTransErr> {
    /// An error occurred while negotiating the transport protocol(s).
    Transport(TransportError<TTransErr>),

    /// The peer identity obtained on the connection did not
    /// match the one that was expected or is otherwise invalid.
    InvalidPeerId,

    /// An I/O error occurred on the connection.
    // TODO: Eventually this should also be a custom error?
    IO(io::Error),

    /// The connection handler produced an error.
    Handler(THandlerErr),
}

impl<THandlerErr, TTransErr> fmt::Display
for ConnectionError<THandlerErr, TTransErr>
where
    THandlerErr: fmt::Display,
    TTransErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionError::IO(err) => write!(f, "I/O error: {}", err),
            ConnectionError::Handler(err) => write!(f, "Handler error: {}", err),
            ConnectionError::Transport(err) => write!(f, "Transport error: {}", err),
            ConnectionError::InvalidPeerId => write!(f, "Invalid peer ID.")
        }
    }
}

impl<THandlerErr, TTransErr> std::error::Error
for ConnectionError<THandlerErr, TTransErr>
where
    THandlerErr: std::error::Error + 'static,
    TTransErr: std::error::Error + 'static
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConnectionError::IO(err) => Some(err),
            ConnectionError::Handler(err) => Some(err),
            ConnectionError::Transport(err) => Some(err),
            ConnectionError::InvalidPeerId { .. } => None,
        }
    }
}

