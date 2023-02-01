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
use libp2p_core::multiaddr::Protocol;
use libp2p_core::multihash::Multihash;
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
pub struct DialOpts {
    peer_id: Option<PeerId>,
    condition: PeerCondition,
    addresses: Vec<Multiaddr>,
    extend_addresses_through_behaviour: bool,
    role_override: Endpoint,
    dial_concurrency_factor_override: Option<NonZeroU8>,
}

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
        self.peer_id
    }

    /// Retrieves the [`PeerId`] from the [`DialOpts`] if specified or otherwise tries to parse it
    /// from the multihash in the `/p2p` part of the address, if present.
    ///
    /// Note: A [`Multiaddr`] with something else other than a [`PeerId`] within the `/p2p` protocol is invalid as per specification.
    /// Unfortunately, we are not making good use of the type system here.
    /// Really, this function should be merged with [`DialOpts::get_peer_id`] above.
    /// If it weren't for the parsing error, the function signatures would be the same.
    ///
    /// See <https://github.com/multiformats/rust-multiaddr/issues/73>.
    pub(crate) fn get_or_parse_peer_id(&self) -> Result<Option<PeerId>, Multihash> {
        if let Some(peer_id) = self.peer_id {
            return Ok(Some(peer_id));
        }

        let first_address = match self.addresses.first() {
            Some(first_address) => first_address,
            None => return Ok(None),
        };

        let maybe_peer_id = first_address
            .iter()
            .last()
            .and_then(|p| {
                if let Protocol::P2p(ma) = p {
                    Some(PeerId::try_from(ma))
                } else {
                    None
                }
            })
            .transpose()?;

        Ok(maybe_peer_id)
    }

    pub(crate) fn get_addresses(&self) -> Vec<Multiaddr> {
        self.addresses.clone()
    }

    pub(crate) fn extend_addresses_through_behaviour(&self) -> bool {
        self.extend_addresses_through_behaviour
    }

    pub(crate) fn peer_condition(&self) -> PeerCondition {
        self.condition
    }

    pub(crate) fn dial_concurrency_override(&self) -> Option<NonZeroU8> {
        self.dial_concurrency_factor_override
    }

    pub(crate) fn role_override(&self) -> Endpoint {
        self.role_override
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

#[derive(Debug)]
pub struct WithPeerId {
    peer_id: PeerId,
    condition: PeerCondition,
    role_override: Endpoint,
    dial_concurrency_factor_override: Option<NonZeroU8>,
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
        DialOpts {
            peer_id: Some(self.peer_id),
            condition: self.condition,
            addresses: vec![],
            extend_addresses_through_behaviour: true,
            role_override: self.role_override,
            dial_concurrency_factor_override: self.dial_concurrency_factor_override,
        }
    }
}

#[derive(Debug)]
pub struct WithPeerIdWithAddresses {
    peer_id: PeerId,
    condition: PeerCondition,
    addresses: Vec<Multiaddr>,
    extend_addresses_through_behaviour: bool,
    role_override: Endpoint,
    dial_concurrency_factor_override: Option<NonZeroU8>,
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
        DialOpts {
            peer_id: Some(self.peer_id),
            condition: self.condition,
            addresses: self.addresses,
            extend_addresses_through_behaviour: self.extend_addresses_through_behaviour,
            role_override: self.role_override,
            dial_concurrency_factor_override: self.dial_concurrency_factor_override,
        }
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
    address: Multiaddr,
    role_override: Endpoint,
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
        DialOpts {
            peer_id: None,
            condition: PeerCondition::Always,
            addresses: vec![self.address],
            extend_addresses_through_behaviour: false,
            role_override: self.role_override,
            dial_concurrency_factor_override: None,
        }
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
#[derive(Debug, Copy, Clone, Default)]
pub enum PeerCondition {
    /// A new dialing attempt is initiated _only if_ the peer is currently
    /// considered disconnected, i.e. there is no established connection
    /// and no ongoing dialing attempt.
    #[default]
    Disconnected,
    /// A new dialing attempt is initiated _only if_ there is currently
    /// no ongoing dialing attempt, i.e. the peer is either considered
    /// disconnected or connected but without an ongoing dialing attempt.
    NotDialing,
    /// A new dialing attempt is always initiated, only subject to the
    /// configured connection limits.
    Always,
}
