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

use libp2p_core::{PeerId, transport};
use std::{fmt, io};

#[derive(Debug)]
pub enum RelayError<E, U> {
    Io(io::Error),
    Transport(transport::Error<E, U>),
    NoRelayFor(PeerId),
    Message(&'static str),
    #[doc(hidden)]
    __Nonexhaustive
}


impl<T, U> fmt::Display for RelayError<T, U>
where
    T: fmt::Display,
    U: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RelayError::Io(e) => write!(f, "i/o error: {}", e),
            RelayError::Transport(e) => write!(f, "transport error: {}", e),
            RelayError::NoRelayFor(p) => write!(f, "no relay for peer: {:?}", p),
            RelayError::Message(m) => write!(f, "{}", m),
            RelayError::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl<T, U> std::error::Error for RelayError<T, U>
where
    T: std::error::Error,
    U: std::error::Error
{
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            RelayError::Io(e) => Some(e),
            RelayError::Transport(e) => Some(e),
            RelayError::NoRelayFor(_) => None,
            RelayError::Message(_) => None,
            RelayError::__Nonexhaustive => None
        }
    }
}

impl<E, U> From<io::Error> for RelayError<E, U> {
    fn from(e: io::Error) -> Self {
        RelayError::Io(e)
    }
}

impl<E, U> From<transport::Error<E, U>> for RelayError<E, U> {
    fn from(e: transport::Error<E, U>) -> Self {
        RelayError::Transport(e)
    }
}

