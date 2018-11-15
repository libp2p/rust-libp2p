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

use multistream_select::ProtocolChoiceError;
use std::fmt;

#[derive(Debug)]
pub enum UpgradeError<E> {
    Select(ProtocolChoiceError),
    Apply(E),
    #[doc(hidden)]
    __Nonexhaustive
}

impl<E> UpgradeError<E>
where
    E: std::error::Error + Send + Sync + 'static
{
    pub fn into_io_error(self) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, self)
    }
}

impl<E> UpgradeError<E> {
    pub fn map_err<F, T>(self, f: F) -> UpgradeError<T>
    where
        F: FnOnce(E) -> T
    {
        match self {
            UpgradeError::Select(e) => UpgradeError::Select(e),
            UpgradeError::Apply(e) => UpgradeError::Apply(f(e)),
            UpgradeError::__Nonexhaustive => UpgradeError::__Nonexhaustive
        }
    }

    pub fn from_err<T>(self) -> UpgradeError<T>
    where
        T: From<E>
    {
        self.map_err(Into::into)
    }
}

impl<E> fmt::Display for UpgradeError<E>
where
    E: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UpgradeError::Select(e) => write!(f, "select error: {}", e),
            UpgradeError::Apply(e) => write!(f, "upgrade apply error: {}", e),
            UpgradeError::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl<E> std::error::Error for UpgradeError<E>
where
    E: std::error::Error
{
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            UpgradeError::Select(e) => Some(e),
            UpgradeError::Apply(e) => Some(e),
            UpgradeError::__Nonexhaustive => None
        }
    }
}

impl<E> From<ProtocolChoiceError> for UpgradeError<E> {
    fn from(e: ProtocolChoiceError) -> Self {
        UpgradeError::Select(e)
    }
}

