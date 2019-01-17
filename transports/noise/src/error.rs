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

use snow::SnowError;
use std::{error::Error, fmt, io};

#[derive(Debug)]
pub enum NoiseError<E> {
    Io(io::Error),
    Noise(SnowError),
    InvalidKey,
    Inner(E),
    #[doc(hidden)]
    __Nonexhaustive
}

impl<E: fmt::Display> fmt::Display for NoiseError<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NoiseError::Io(e) => write!(f, "i/o: {:?}", e),
            NoiseError::Noise(e) => write!(f, "noise: {:?}", e),
            NoiseError::Inner(e) => write!(f, "{}", e),
            NoiseError::InvalidKey => f.write_str("invalid public key"),
            NoiseError::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl<E: Error + 'static> Error for NoiseError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            NoiseError::Io(e) => Some(e),
            NoiseError::Inner(e) => Some(e),
            NoiseError::Noise(_) => None, // TODO: `SnowError` should implement `Error`.
            NoiseError::InvalidKey => None,
            NoiseError::__Nonexhaustive => None
        }
    }
}

impl<E> From<io::Error> for NoiseError<E> {
    fn from(e: io::Error) -> Self {
        NoiseError::Io(e)
    }
}

impl<E> From<SnowError> for NoiseError<E> {
    fn from(e: SnowError) -> Self {
        NoiseError::Noise(e)
    }
}
