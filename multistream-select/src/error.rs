// Copyright 2017 Parity Technologies (UK) Ltd.

// Libp2p-rs is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Libp2p-rs is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Libp2p-rs.  If not, see <http://www.gnu.org/licenses/>.

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
