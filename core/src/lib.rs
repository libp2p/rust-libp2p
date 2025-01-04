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
//! - The [`Transport`] trait defines how to reach a remote node or listen for incoming remote
//!   connections. See the [`transport`] module.
//! - The [`StreamMuxer`] trait is implemented on structs that hold a connection to a remote and can
//!   subdivide this connection into multiple substreams. See the [`muxing`] module.
//! - The [`UpgradeInfo`], [`InboundUpgrade`] and [`OutboundUpgrade`] traits define how to upgrade
//!   each individual substream to use a protocol. See the `upgrade` module.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub use self::{
        envelope_proto::*,
        peer_record_proto::{mod_PeerRecord::*, PeerRecord},
    };
}

/// Multi-address re-export.
pub use multiaddr;
pub type Negotiated<T> = multistream_select::Negotiated<T>;

pub mod connection;
pub mod either;
pub mod muxing;
pub mod peer_record;
pub mod signed_envelope;
pub mod transport;
pub mod upgrade;

pub use connection::{ConnectedPoint, Endpoint};
pub use libp2p_identity::PeerId;
pub use multiaddr::Multiaddr;
pub use multihash;
pub use muxing::StreamMuxer;
pub use peer_record::PeerRecord;
pub use signed_envelope::SignedEnvelope;
pub use transport::Transport;
pub use upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct DecodeError(quick_protobuf::Error);

pub mod util {
    use std::convert::Infallible;

    /// A safe version of [`std::intrinsics::unreachable`].
    #[inline(always)]
    pub fn unreachable(x: Infallible) -> ! {
        match x {}
    }
}
