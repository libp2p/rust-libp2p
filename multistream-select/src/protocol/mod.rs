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
