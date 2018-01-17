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

use std::error;
use std::fmt;
use std::io::Error as IoError;

#[derive(Debug)]
pub enum KadError {
    IoError(IoError),

    // TODO: this is a temporary general purpose error
    Failure,
}

impl From<IoError> for KadError {
    #[inline]
    fn from(err: IoError) -> KadError {
        KadError::IoError(err)
    }
}

impl error::Error for KadError {
	#[inline]
	fn description(&self) -> &str {
		match *self {
			KadError::IoError(_) => {
				"I/O error"
			},
            KadError::Failure => {
                "general-purpose failure error"
            },
		}
	}

	fn cause(&self) -> Option<&error::Error> {
		match *self {
			KadError::IoError(ref err) => {
				Some(err)
			}
			_ => None,
		}
	}
}

impl fmt::Display for KadError {
	#[inline]
	fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		write!(fmt, "{}", error::Error::description(self))
	}
}
