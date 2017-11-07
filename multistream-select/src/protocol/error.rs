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

//! Contains the error structs for the low-level protocol handling.

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
