// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Transports, upgrades, multiplexing and node handling of *libp2p*.
//!
//! The main concepts of libp2p-core are:
//!
//! - A [`PeerId`] is a unique global identifier for a node on the network.
//!   Each node must have a different [`PeerId`]. Normally, a [`PeerId`] is the
//!   hash of the public key used to negotiate encryption on the
//!   communication channel, thereby guaranteeing that they cannot be spoofed.
//! - The [`Transport`] trait defines how to reach a remote node or listen for
//!   incoming remote connections. See the [`transport`] module.
//! - The [`StreamMuxer`] trait is implemented on structs that hold a connection
//!   to a remote and can subdivide this connection into multiple substreams.
//!   See the [`muxing`] module.
//! - The [`UpgradeInfo`], [`InboundUpgrade`] and [`OutboundUpgrade`] traits
//!   define how to upgrade each individual substream to use a protocol.
//!   See the `upgrade` module.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod proto {
    include!("generated/mod.rs");
    pub use self::{
        envelope_proto::*, keys_proto::*, peer_record_proto::mod_PeerRecord::*,
        peer_record_proto::PeerRecord,
    };
}

/// Multi-address re-export.
pub use multiaddr;
use std::fmt;
use std::fmt::Formatter;
pub type Negotiated<T> = multistream_select::Negotiated<T>;

mod peer_id;
mod translation;

pub mod connection;
pub mod either;
pub mod identity;
pub mod muxing;
pub mod peer_record;
pub mod signed_envelope;
pub mod transport;
pub mod upgrade;

pub use connection::{ConnectedPoint, Endpoint};
pub use identity::PublicKey;
pub use multiaddr::Multiaddr;
pub use multihash;
pub use muxing::StreamMuxer;
pub use peer_id::ParseError;
pub use peer_id::PeerId;
pub use peer_record::PeerRecord;
pub use signed_envelope::SignedEnvelope;
pub use translation::address_translation;
pub use transport::Transport;
pub use upgrade::{InboundUpgrade, OutboundUpgrade, ProtocolName, UpgradeError, UpgradeInfo};

#[derive(Debug, thiserror::Error)]
pub struct DecodeError(String);

impl From<quick_protobuf::Error> for DecodeError {
    fn from(e: quick_protobuf::Error) -> Self {
        Self(e.to_string())
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
