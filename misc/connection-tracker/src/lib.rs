//! A [`NetworkBehaviour`] for tracking connected peers.
//!
//! This crate provides simple connected peer tracking that can be composed
//! with other behaviours to avoid manual connection state management.
//!
//! # Usage
//!
//! ```rust
//! use libp2p_swarm::{dummy, NetworkBehaviour};//!
//!
//! use libp2p_swarm::dummy::Behaviour;
//!
//! #[derive(NetworkBehaviour)]
//! #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
//! struct MyBehaviour {
//!     connection_tracker: Behaviour,
//!     // ... other behaviours
//! }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod behavior;
mod store;

use libp2p_core::{ConnectedPoint, PeerId};
use libp2p_swarm::ConnectionId;

/// Events emitted by the connection tracker behaviour.
#[derive(Debug, Clone)]
pub enum Event {
    /// A peer connected (first connection established).
    PeerConnected {
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    },

    /// A peer disconnected (last connection closed).
    PeerDisconnected {
        peer_id: PeerId,
        connection_id: ConnectionId,
    },
}
