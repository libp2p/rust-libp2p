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

//! Main `ProtocolChoiceError` error.

use protocol::MultistreamSelectError;
use std::io::Error as IoError;

/// Error that can happen when negociating a protocol with the remote.
#[derive(Debug)]
pub enum ProtocolChoiceError {
	/// Error in the protocol.
	MultistreamSelectError(MultistreamSelectError),

	/// Received a message from the remote that makes no sense in the current context.
	UnexpectedMessage,

	/// We don't support any protocol in common with the remote.
	NoProtocolFound,
}

impl From<MultistreamSelectError> for ProtocolChoiceError {
	#[inline]
	fn from(err: MultistreamSelectError) -> ProtocolChoiceError {
		ProtocolChoiceError::MultistreamSelectError(err)
	}
}

impl From<IoError> for ProtocolChoiceError {
	#[inline]
	fn from(err: IoError) -> ProtocolChoiceError {
		MultistreamSelectError::from(err).into()
	}
}
