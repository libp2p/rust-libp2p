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
pub enum Error<E> {
    Select(ProtocolChoiceError),
    Apply(E),
    #[doc(hidden)]
    __Nonexhaustive
}

impl<E> fmt::Display for Error<E>
where
    E: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Select(e) => write!(f, "select error: {}", e),
            Error::Apply(e) => write!(f, "upgrade apply error: {}", e),
            Error::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl<E> std::error::Error for Error<E>
where
    E: std::error::Error
{
    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self {
            Error::Select(e) => Some(e),
            Error::Apply(e) => Some(e),
            Error::__Nonexhaustive => None
        }
    }
}

impl<E> From<ProtocolChoiceError> for Error<E> {
    fn from(e: ProtocolChoiceError) -> Self {
        Error::Select(e)
    }
}

