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

use libp2p_core::{PeerId, upgrade::UpgradeError};
use std::{fmt, io};

#[derive(Debug)]
pub enum RelayError<E> {
    Io(io::Error),
    Upgrade(UpgradeError<E>),
    NoRelayFor(PeerId),
    Message(&'static str),
    #[doc(hidden)]
    __Nonexhaustive
}


impl<E> fmt::Display for RelayError<E>
where
    E: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RelayError::Io(e) => write!(f, "i/o error: {}", e),
            RelayError::Upgrade(e) => write!(f, "upgrade error: {}", e),
            RelayError::NoRelayFor(p) => write!(f, "no relay for peer: {:?}", p),
            RelayError::Message(m) => write!(f, "{}", m),
            RelayError::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl<E> std::error::Error for RelayError<E>
where
    E: std::error::Error
{
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            RelayError::Io(e) => Some(e),
            RelayError::Upgrade(e) => Some(e),
            RelayError::NoRelayFor(_) => None,
            RelayError::Message(_) => None,
            RelayError::__Nonexhaustive => None
        }
    }
}

impl<E> From<io::Error> for RelayError<E> {
    fn from(e: io::Error) -> Self {
        RelayError::Io(e)
    }
}

impl<E> From<UpgradeError<E>> for RelayError<E> {
    fn from(e: UpgradeError<E>) -> Self {
        RelayError::Upgrade(e)
    }
}

