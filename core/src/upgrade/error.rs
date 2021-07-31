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

use multistream_select::NegotiationError;
use std::fmt;

/// Error that can happen when upgrading a connection or substream to use a protocol.
#[derive(Debug)]
pub enum UpgradeError<E> {
    /// Error during the negotiation process.
    Select(NegotiationError),
    /// Error during the post-negotiation handshake.
    Apply(E),
}

impl<E> UpgradeError<E> {
    pub fn map_err<F, T>(self, f: F) -> UpgradeError<T>
    where
        F: FnOnce(E) -> T,
    {
        match self {
            UpgradeError::Select(e) => UpgradeError::Select(e),
            UpgradeError::Apply(e) => UpgradeError::Apply(f(e)),
        }
    }

    pub fn into_err<T>(self) -> UpgradeError<T>
    where
        T: From<E>,
    {
        self.map_err(Into::into)
    }
}

impl<E> fmt::Display for UpgradeError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpgradeError::Select(e) => write!(f, "select error: {}", e),
            UpgradeError::Apply(e) => write!(f, "upgrade apply error: {}", e),
        }
    }
}

impl<E> std::error::Error for UpgradeError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            UpgradeError::Select(e) => Some(e),
            UpgradeError::Apply(e) => Some(e),
        }
    }
}

impl<E> From<NegotiationError> for UpgradeError<E> {
    fn from(e: NegotiationError) -> Self {
        UpgradeError::Select(e)
    }
}
