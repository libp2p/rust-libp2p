// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Contains the error structs for the low-level protocol handling.

use std::error;
use std::fmt;
use std::io::Error as IoError;

/// Error at the multistream-select layer of communication.
#[derive(Debug)]
pub enum MultistreamSelectError {
	/// I/O error.
	IoError(IoError),

	/// The remote doesn't use the same multistream-select protocol as we do.
	FailedHandshake,

	/// Received an unknown message from the remote.
	UnknownMessage,

	/// Protocol names must always start with `/`, otherwise this error is returned.
	WrongProtocolName,
}

impl From<IoError> for MultistreamSelectError {
	#[inline]
	fn from(err: IoError) -> MultistreamSelectError {
		MultistreamSelectError::IoError(err)
	}
}

impl error::Error for MultistreamSelectError {
	#[inline]
	fn description(&self) -> &str {
		match *self {
			MultistreamSelectError::IoError(_) => {
				"I/O error"
			},
			MultistreamSelectError::FailedHandshake => {
				"the remote doesn't use the same multistream-select protocol as we do"
			},
			MultistreamSelectError::UnknownMessage => {
				"received an unknown message from the remote"
			},
			MultistreamSelectError::WrongProtocolName => {
				"protocol names must always start with `/`, otherwise this error is returned"
			},
		}
	}

	fn cause(&self) -> Option<&error::Error> {
		match *self {
			MultistreamSelectError::IoError(ref err) => {
				Some(err)
			}
			_ => None,
		}
	}
}

impl fmt::Display for MultistreamSelectError {
	#[inline]
	fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
		write!(fmt, "{}", error::Error::description(self))
	}
}
