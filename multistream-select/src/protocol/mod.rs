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

//! Contains lower-level structs to handle the multistream protocol.

use bytes::Bytes;

mod dialer;
mod error;
mod listener;

const MULTISTREAM_PROTOCOL_WITH_LF: &'static [u8] = b"/multistream/1.0.0\n";

pub use self::dialer::Dialer;
pub use self::error::MultistreamSelectError;
pub use self::listener::Listener;

/// Message sent from the dialer to the listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialerToListenerMessage {
	/// The dialer wants us to use a protocol.
	///
	/// If this is accepted (by receiving back a `ProtocolAck`), then we immediately start
	/// communicating in the new protocol.
	ProtocolRequest {
		/// Name of the protocol.
		name: Bytes,
	},

	/// The dialer requested the list of protocols that the listener supports.
	ProtocolsListRequest,
}

/// Message sent from the listener to the dialer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerToDialerMessage {
	/// The protocol requested by the dialer is accepted. The socket immediately starts using the
	/// new protocol.
	ProtocolAck { name: Bytes },

	/// The protocol requested by the dialer is not supported or available.
	NotAvailable,

	/// Response to the request for the list of protocols.
	ProtocolsListResponse {
		/// The list of protocols.
		// TODO: use some sort of iterator
		list: Vec<Bytes>,
	},
}
