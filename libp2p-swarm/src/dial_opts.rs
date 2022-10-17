// Copyright 2019 Parity Technologies (UK) Ltd.
// Copyright 2021 Protocol Labs.
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

use libp2p_core::connection::Endpoint;
use libp2p_core::{Multiaddr, PeerId};
use std::num::NonZeroU8;

/// Options to configure a dial to a known or unknown peer.
///
/// Used in [`Swarm::dial`](crate::Swarm::dial) and
/// [`NetworkBehaviourAction::Dial`](crate::behaviour::NetworkBehaviourAction::Dial).
///
/// To construct use either of:
///
/// - [`DialOpts::peer_id`] dialing a known peer
///
/// - [`DialOpts::unknown_peer_id`] dialing an unknown peer
#[derive(Debug)]
pub struct DialOpts(pub(super) Opts);

impl DialOpts {
    /// Dial a known peer.
    ///
    ///   ```
    ///   # use libp2p_swarm::dial_opts::{DialOpts, PeerCondition};
    ///   # use libp2p_core::PeerId;
    ///   DialOpts::peer_id(PeerId::random())
    ///      .condition(PeerCondition::Disconnected)
    ///      .addresses(vec!["/ip6/::1/tcp/12345".parse().unwrap()])
    ///      .extend_addresses_through_behaviour()
    ///      .build();
    ///   ```
    pub fn peer_id(peer_id: PeerId) -> WithPeerId {
        WithPeerId {
            peer_id,
            condition: Default::default(),
            role_override: Endpoint::Dialer,
            dial_concurrency_factor_override: Default::default(),
        }
    }

    /// Dial an unknown peer.
    ///
    ///   ```
    ///   # use libp2p_swarm::dial_opts::DialOpts;
    ///   DialOpts::unknown_peer_id()
    ///      .address("/ip6/::1/tcp/12345".parse().unwrap())
    ///      .build();
    ///   ```
    pub fn unknown_peer_id() -> WithoutPeerId {
        WithoutPeerId {}
    }

    /// Get the [`PeerId`] specified in a [`DialOpts`] if any.
    pub fn get_peer_id(&self) -> Option<PeerId> {
        match self {
            DialOpts(Opts::WithPeerId(WithPeerId { peer_id, .. })) => Some(*peer_id),
            DialOpts(Opts::WithPeerIdWithAddresses(WithPeerIdWithAddresses {
                peer_id, ..
            })) => Some(*peer_id),
            DialOpts(Opts::WithoutPeerIdWithAddress(_)) => None,
        }
    }
}

impl From<Multiaddr> for DialOpts {
    fn from(address: Multiaddr) -> Self {
        DialOpts::unknown_peer_id().address(address).build()
    }
}

impl From<PeerId> for DialOpts {
    fn from(peer_id: PeerId) -> Self {
        DialOpts::peer_id(peer_id).build()
    }
}

/// Internal options type.
///
/// Not to be constructed manually. Use either of the below instead:
///
/// - [`DialOpts::peer_id`] dialing a known peer
/// - [`DialOpts::unknown_peer_id`] dialing an unknown peer
#[derive(Debug)]
pub(super) enum Opts {
    WithPeerId(WithPeerId),
    WithPeerIdWithAddresses(WithPeerIdWithAddresses),
    WithoutPeerIdWithAddress(WithoutPeerIdWithAddress),
}

#[derive(Debug)]
pub struct WithPeerId {
    pub(crate) peer_id: PeerId,
    pub(crate) condition: PeerCondition,
    pub(crate) role_override: Endpoint,
    pub(crate) dial_concurrency_factor_override: Option<NonZeroU8>,
}

impl WithPeerId {
    /// Specify a [`PeerCondition`] for the dial.
    pub fn condition(mut self, condition: PeerCondition) -> Self {
        self.condition = condition;
        self
    }

    /// Override
    /// Number of addresses concurrently dialed for a single outbound connection attempt.
    pub fn override_dial_concurrency_factor(mut self, factor: NonZeroU8) -> Self {
        self.dial_concurrency_factor_override = Some(factor);
        self
    }

    /// Specify a set of addresses to be used to dial the known peer.
    pub fn addresses(self, addresses: Vec<Multiaddr>) -> WithPeerIdWithAddresses {
        WithPeerIdWithAddresses {
            peer_id: self.peer_id,
            condition: self.condition,
            addresses,
            extend_addresses_through_behaviour: false,
            role_override: self.role_override,
            dial_concurrency_factor_override: self.dial_concurrency_factor_override,
        }
    }

    /// Override role of local node on connection. I.e. execute the dial _as a
    /// listener_.
    ///
    /// See
    /// [`ConnectedPoint::Dialer`](libp2p_core::connection::ConnectedPoint::Dialer)
    /// for details.
    pub fn override_role(mut self) -> Self {
        self.role_override = Endpoint::Listener;
        self
    }

    /// Build the final [`DialOpts`].
    ///
    /// Addresses to dial the peer are retrieved via
    /// [`NetworkBehaviour::addresses_of_peer`](crate::behaviour::NetworkBehaviour::addresses_of_peer).
    pub fn build(self) -> DialOpts {
        DialOpts(Opts::WithPeerId(self))
    }
}

#[derive(Debug)]
pub struct WithPeerIdWithAddresses {
    pub(crate) peer_id: PeerId,
    pub(crate) condition: PeerCondition,
    pub(crate) addresses: Vec<Multiaddr>,
    pub(crate) extend_addresses_through_behaviour: bool,
    pub(crate) role_override: Endpoint,
    pub(crate) dial_concurrency_factor_override: Option<NonZeroU8>,
}

impl WithPeerIdWithAddresses {
    /// Specify a [`PeerCondition`] for the dial.
    pub fn condition(mut self, condition: PeerCondition) -> Self {
        self.condition = condition;
        self
    }

    /// In addition to the provided addresses, extend the set via
    /// [`NetworkBehaviour::addresses_of_peer`](crate::behaviour::NetworkBehaviour::addresses_of_peer).
    pub fn extend_addresses_through_behaviour(mut self) -> Self {
        self.extend_addresses_through_behaviour = true;
        self
    }

    /// Override role of local node on connection. I.e. execute the dial _as a
    /// listener_.
    ///
    /// See
    /// [`ConnectedPoint::Dialer`](libp2p_core::connection::ConnectedPoint::Dialer)
    /// for details.
    pub fn override_role(mut self) -> Self {
        self.role_override = Endpoint::Listener;
        self
    }

    /// Override
    /// Number of addresses concurrently dialed for a single outbound connection attempt.
    pub fn override_dial_concurrency_factor(mut self, factor: NonZeroU8) -> Self {
        self.dial_concurrency_factor_override = Some(factor);
        self
    }

    /// Build the final [`DialOpts`].
    pub fn build(self) -> DialOpts {
        DialOpts(Opts::WithPeerIdWithAddresses(self))
    }
}

#[derive(Debug)]
pub struct WithoutPeerId {}

impl WithoutPeerId {
    /// Specify a single address to dial the unknown peer.
    pub fn address(self, address: Multiaddr) -> WithoutPeerIdWithAddress {
        WithoutPeerIdWithAddress {
            address,
            role_override: Endpoint::Dialer,
        }
    }
}

#[derive(Debug)]
pub struct WithoutPeerIdWithAddress {
    pub(crate) address: Multiaddr,
    pub(crate) role_override: Endpoint,
}

impl WithoutPeerIdWithAddress {
    /// Override role of local node on connection. I.e. execute the dial _as a
    /// listener_.
    ///
    /// See
    /// [`ConnectedPoint::Dialer`](libp2p_core::connection::ConnectedPoint::Dialer)
    /// for details.
    pub fn override_role(mut self) -> Self {
        self.role_override = Endpoint::Listener;
        self
    }
    /// Build the final [`DialOpts`].
    pub fn build(self) -> DialOpts {
        DialOpts(Opts::WithoutPeerIdWithAddress(self))
    }
}

/// The available conditions under which a new dialing attempt to
/// a known peer is initiated.
///
/// ```
/// # use libp2p_swarm::dial_opts::{DialOpts, PeerCondition};
/// # use libp2p_core::PeerId;
/// #
/// DialOpts::peer_id(PeerId::random())
///    .condition(PeerCondition::Disconnected)
///    .build();
/// ```
#[derive(Debug, Copy, Clone)]
pub enum PeerCondition {
    /// A new dialing attempt is initiated _only if_ the peer is currently
    /// considered disconnected, i.e. there is no established connection
    /// and no ongoing dialing attempt.
    Disconnected,
    /// A new dialing attempt is initiated _only if_ there is currently
    /// no ongoing dialing attempt, i.e. the peer is either considered
    /// disconnected or connected but without an ongoing dialing attempt.
    NotDialing,
    /// A new dialing attempt is always initiated, only subject to the
    /// configured connection limits.
    Always,
}

impl Default for PeerCondition {
    fn default() -> Self {
        PeerCondition::Disconnected
    }
}
