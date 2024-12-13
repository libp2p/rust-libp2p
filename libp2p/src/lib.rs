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

//! libp2p is a modular peer-to-peer networking framework.
//!
//! To learn more about the general libp2p multi-language framework visit <https://libp2p.io>.
//!
//! To get started with this libp2p implementation in Rust, please take a look
//! at the [`tutorials`]. Further examples can be found in the
//! [examples] directory.
//!
//! [examples]: https://github.com/libp2p/rust-libp2p/tree/master/examples

#![doc(html_logo_url = "https://libp2p.io/img/logo_small.png")]
#![doc(html_favicon_url = "https://libp2p.io/img/favicon.png")]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub use bytes;
pub use futures;
#[doc(inline)]
pub use libp2p_allow_block_list as allow_block_list;
#[cfg(feature = "autonat")]
#[doc(inline)]
pub use libp2p_autonat as autonat;
#[doc(inline)]
pub use libp2p_connection_limits as connection_limits;
#[doc(inline)]
pub use libp2p_core as core;
#[doc(inline)]
pub use libp2p_core::multihash;
#[cfg(feature = "dcutr")]
#[doc(inline)]
pub use libp2p_dcutr as dcutr;
#[cfg(feature = "dns")]
#[cfg_attr(docsrs, doc(cfg(feature = "dns")))]
#[cfg(not(target_arch = "wasm32"))]
#[doc(inline)]
pub use libp2p_dns as dns;
#[cfg(feature = "floodsub")]
#[doc(inline)]
pub use libp2p_floodsub as floodsub;
#[cfg(feature = "gossipsub")]
#[doc(inline)]
pub use libp2p_gossipsub as gossipsub;
#[cfg(feature = "identify")]
#[doc(inline)]
pub use libp2p_identify as identify;
#[cfg(feature = "kad")]
#[doc(inline)]
pub use libp2p_kad as kad;
#[cfg(feature = "mdns")]
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(docsrs, doc(cfg(feature = "mdns")))]
#[doc(inline)]
pub use libp2p_mdns as mdns;
#[cfg(feature = "memory-connection-limits")]
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(docsrs, doc(cfg(feature = "memory-connection-limits")))]
#[doc(inline)]
pub use libp2p_memory_connection_limits as memory_connection_limits;
#[cfg(feature = "metrics")]
#[doc(inline)]
pub use libp2p_metrics as metrics;
#[cfg(feature = "noise")]
#[doc(inline)]
pub use libp2p_noise as noise;
#[cfg(feature = "ping")]
#[doc(inline)]
pub use libp2p_ping as ping;
#[cfg(feature = "plaintext")]
#[doc(inline)]
pub use libp2p_plaintext as plaintext;
#[cfg(feature = "pnet")]
#[doc(inline)]
pub use libp2p_pnet as pnet;
#[cfg(feature = "quic")]
#[cfg(not(target_arch = "wasm32"))]
pub use libp2p_quic as quic;
#[cfg(feature = "relay")]
#[doc(inline)]
pub use libp2p_relay as relay;
#[cfg(feature = "rendezvous")]
#[doc(inline)]
pub use libp2p_rendezvous as rendezvous;
#[cfg(feature = "request-response")]
#[doc(inline)]
pub use libp2p_request_response as request_response;
#[doc(inline)]
pub use libp2p_swarm as swarm;
#[cfg(feature = "tcp")]
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(docsrs, doc(cfg(feature = "tcp")))]
#[doc(inline)]
pub use libp2p_tcp as tcp;
#[cfg(feature = "tls")]
#[cfg_attr(docsrs, doc(cfg(feature = "tls")))]
#[cfg(not(target_arch = "wasm32"))]
#[doc(inline)]
pub use libp2p_tls as tls;
#[cfg(feature = "uds")]
#[cfg_attr(docsrs, doc(cfg(feature = "uds")))]
#[cfg(not(target_arch = "wasm32"))]
#[doc(inline)]
pub use libp2p_uds as uds;
#[cfg(feature = "upnp")]
#[cfg(not(target_arch = "wasm32"))]
#[doc(inline)]
pub use libp2p_upnp as upnp;
#[cfg(feature = "websocket")]
#[cfg(not(target_arch = "wasm32"))]
#[doc(inline)]
pub use libp2p_websocket as websocket;
#[cfg(feature = "websocket-websys")]
#[doc(inline)]
pub use libp2p_websocket_websys as websocket_websys;
#[cfg(feature = "webtransport-websys")]
#[cfg_attr(docsrs, doc(cfg(feature = "webtransport-websys")))]
#[doc(inline)]
pub use libp2p_webtransport_websys as webtransport_websys;
#[cfg(feature = "yamux")]
#[doc(inline)]
pub use libp2p_yamux as yamux;
#[doc(inline)]
pub use multiaddr;

mod builder;
mod transport_ext;

pub mod bandwidth;

#[cfg(doc)]
pub mod tutorials;

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
pub use builder::WebsocketError as WebsocketBuilderError;
pub use libp2p_identity as identity;
pub use libp2p_identity::PeerId;
pub use libp2p_swarm::{Stream, StreamProtocol};

pub use self::{
    builder::{
        BehaviourError as BehaviourBuilderError, SwarmBuilder,
        TransportError as TransportBuilderError,
    },
    core::{
        transport::TransportError,
        upgrade::{InboundUpgrade, OutboundUpgrade},
        Transport,
    },
    multiaddr::{multiaddr as build_multiaddr, Multiaddr},
    swarm::Swarm,
    transport_ext::TransportExt,
};
