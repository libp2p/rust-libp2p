// Copyright 2023 Protocol Labs.
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

//! A libp2p module for managing allow and blocks lists to peers.
//!
//! # Allow list example
//!
//! ```rust
//! # use libp2p_swarm::Swarm;
//! # use libp2p_swarm_derive::NetworkBehaviour;
//! # use libp2p_allow_block_list as allow_block_list;
//! # use libp2p_allow_block_list::AllowedPeers;
//! #
//! #[derive(NetworkBehaviour)]
//! # #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
//! struct MyBehaviour {
//!    allowed_peers: allow_block_list::Behaviour<AllowedPeers>,
//! }
//!
//! # fn main() {
//! let behaviour = MyBehaviour {
//!     allowed_peers: allow_block_list::Behaviour::default()
//! };
//! # }
//! ```
//! # Block list example
//!
//! ```rust
//! # use libp2p_swarm::Swarm;
//! # use libp2p_swarm_derive::NetworkBehaviour;
//! # use libp2p_allow_block_list as allow_block_list;
//! # use libp2p_allow_block_list::BlockedPeers;
//! #
//! #[derive(NetworkBehaviour)]
//! # #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
//! struct MyBehaviour {
//!    blocked_peers: allow_block_list::Behaviour<BlockedPeers>,
//! }
//!
//! # fn main() {
//! let behaviour = MyBehaviour {
//!     blocked_peers: allow_block_list::Behaviour::default()
//! };
//! # }
//! ```

use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dummy, CloseConnection, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour,
    PollParameters, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::task::{Context, Poll, Waker};
use void::Void;

/// A [`NetworkBehaviour`] that can act as an allow or block list.
#[derive(Default, Debug)]
pub struct Behaviour<S> {
    state: S,
    close_connections: VecDeque<PeerId>,
    waker: Option<Waker>,
}

/// The list of explicitly allowed peers.
#[derive(Default)]
pub struct AllowedPeers {
    peers: HashSet<PeerId>,
}

/// The list of explicitly blocked peers.
#[derive(Default)]
pub struct BlockedPeers {
    peers: HashSet<PeerId>,
}

impl Behaviour<AllowedPeers> {
    /// Allow connections to the given peer.
    pub fn allow_peer(&mut self, peer: PeerId) {
        self.state.peers.insert(peer);
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }

    /// Disallow connections to the given peer.
    ///
    /// All active connections to this peer will be closed immediately.
    pub fn disallow_peer(&mut self, peer: PeerId) {
        self.state.peers.remove(&peer);
        self.close_connections.push_back(peer);
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }
}

impl Behaviour<BlockedPeers> {
    /// Block connections to a given peer.
    ///
    /// All active connections to this peer will be closed immediately.
    pub fn block_peer(&mut self, peer: PeerId) {
        self.state.peers.insert(peer);
        self.close_connections.push_back(peer);
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }

    /// Unblock connections to a given peer.
    pub fn unblock_peer(&mut self, peer: PeerId) {
        self.state.peers.remove(&peer);
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }
}

/// A connection to this peer is not explicitly allowed and was thus [`denied`](ConnectionDenied).
#[derive(Debug)]
pub struct NotAllowed {
    peer: PeerId,
}

impl fmt::Display for NotAllowed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer {} is not in the allow list", self.peer)
    }
}

impl std::error::Error for NotAllowed {}

/// A connection to this peer was explicitly blocked and was thus [`denied`](ConnectionDenied).
#[derive(Debug)]
pub struct Blocked {
    peer: PeerId,
}

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer {} is in the block list", self.peer)
    }
}

impl std::error::Error for Blocked {}

trait Enforce: 'static {
    fn enforce(&self, peer: &PeerId) -> Result<(), ConnectionDenied>;
}

impl Enforce for AllowedPeers {
    fn enforce(&self, peer: &PeerId) -> Result<(), ConnectionDenied> {
        if !self.peers.contains(peer) {
            return Err(ConnectionDenied::new(NotAllowed { peer: *peer }));
        }

        Ok(())
    }
}

impl Enforce for BlockedPeers {
    fn enforce(&self, peer: &PeerId) -> Result<(), ConnectionDenied> {
        if self.peers.contains(peer) {
            return Err(ConnectionDenied::new(Blocked { peer: *peer }));
        }

        Ok(())
    }
}

impl<S> NetworkBehaviour for Behaviour<S>
where
    S: Enforce,
{
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Void;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.state.enforce(&peer)?;

        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        peer: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        if let Some(peer) = peer {
            self.state.enforce(&peer)?;
        }

        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.state.enforce(&peer)?;

        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionClosed(_) => {}
            FromSwarm::ConnectionEstablished(_) => {}
            FromSwarm::AddressChange(_) => {}
            FromSwarm::DialFailure(_) => {}
            FromSwarm::ListenFailure(_) => {}
            FromSwarm::NewListener(_) => {}
            FromSwarm::NewListenAddr(_) => {}
            FromSwarm::ExpiredListenAddr(_) => {}
            FromSwarm::ListenerError(_) => {}
            FromSwarm::ListenerClosed(_) => {}
            FromSwarm::NewExternalAddrCandidate(_) => {}
            FromSwarm::ExternalAddrExpired(_) => {}
            FromSwarm::ExternalAddrConfirmed(_) => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(peer) = self.close_connections.pop_front() {
            return Poll::Ready(ToSwarm::CloseConnection {
                peer_id: peer,
                connection: CloseConnection::All,
            });
        }

        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_swarm::{dial_opts::DialOpts, DialError, ListenError, Swarm, SwarmEvent};
    use libp2p_swarm_test::SwarmExt;

    #[async_std::test]
    async fn cannot_dial_blocked_peer() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        listener.listen().await;

        dialer
            .behaviour_mut()
            .list
            .block_peer(*listener.local_peer_id());

        let DialError::Denied { cause } = dial(&mut dialer, &listener).unwrap_err() else {
            panic!("unexpected dial error")
        };
        assert!(cause.downcast::<Blocked>().is_ok());
    }

    #[async_std::test]
    async fn can_dial_unblocked_peer() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        listener.listen().await;

        dialer
            .behaviour_mut()
            .list
            .block_peer(*listener.local_peer_id());
        dialer
            .behaviour_mut()
            .list
            .unblock_peer(*listener.local_peer_id());

        dial(&mut dialer, &listener).unwrap();
    }

    #[async_std::test]
    async fn blocked_peer_cannot_dial_us() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        listener.listen().await;

        listener
            .behaviour_mut()
            .list
            .block_peer(*dialer.local_peer_id());
        dial(&mut dialer, &listener).unwrap();
        async_std::task::spawn(dialer.loop_on_next());

        let cause = listener
            .wait(|e| match e {
                SwarmEvent::IncomingConnectionError {
                    error: ListenError::Denied { cause },
                    ..
                } => Some(cause),
                _ => None,
            })
            .await;
        assert!(cause.downcast::<Blocked>().is_ok());
    }

    #[async_std::test]
    async fn connections_get_closed_upon_blocked() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<BlockedPeers>::new());
        listener.listen().await;
        dialer.connect(&mut listener).await;

        dialer
            .behaviour_mut()
            .list
            .block_peer(*listener.local_peer_id());

        let (
            [SwarmEvent::ConnectionClosed {
                peer_id: closed_dialer_peer,
                ..
            }],
            [SwarmEvent::ConnectionClosed {
                peer_id: closed_listener_peer,
                ..
            }],
        ) = libp2p_swarm_test::drive(&mut dialer, &mut listener).await
        else {
            panic!("unexpected events")
        };
        assert_eq!(closed_dialer_peer, *listener.local_peer_id());
        assert_eq!(closed_listener_peer, *dialer.local_peer_id());
    }

    #[async_std::test]
    async fn cannot_dial_peer_unless_allowed() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        listener.listen().await;

        let DialError::Denied { cause } = dial(&mut dialer, &listener).unwrap_err() else {
            panic!("unexpected dial error")
        };
        assert!(cause.downcast::<NotAllowed>().is_ok());

        dialer
            .behaviour_mut()
            .list
            .allow_peer(*listener.local_peer_id());
        assert!(dial(&mut dialer, &listener).is_ok());
    }

    #[async_std::test]
    async fn cannot_dial_disallowed_peer() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        listener.listen().await;

        dialer
            .behaviour_mut()
            .list
            .allow_peer(*listener.local_peer_id());
        dialer
            .behaviour_mut()
            .list
            .disallow_peer(*listener.local_peer_id());

        let DialError::Denied { cause } = dial(&mut dialer, &listener).unwrap_err() else {
            panic!("unexpected dial error")
        };
        assert!(cause.downcast::<NotAllowed>().is_ok());
    }

    #[async_std::test]
    async fn not_allowed_peer_cannot_dial_us() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        listener.listen().await;

        dialer
            .dial(
                DialOpts::unknown_peer_id()
                    .address(listener.external_addresses().next().cloned().unwrap())
                    .build(),
            )
            .unwrap();

        let (
            [SwarmEvent::OutgoingConnectionError {
                error:
                    DialError::Denied {
                        cause: outgoing_cause,
                    },
                ..
            }],
            [_, SwarmEvent::IncomingConnectionError {
                error:
                    ListenError::Denied {
                        cause: incoming_cause,
                    },
                ..
            }],
        ) = libp2p_swarm_test::drive(&mut dialer, &mut listener).await
        else {
            panic!("unexpected events")
        };
        assert!(outgoing_cause.downcast::<NotAllowed>().is_ok());
        assert!(incoming_cause.downcast::<NotAllowed>().is_ok());
    }

    #[async_std::test]
    async fn connections_get_closed_upon_disallow() {
        let mut dialer = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        let mut listener = Swarm::new_ephemeral(|_| Behaviour::<AllowedPeers>::new());
        listener.listen().await;
        dialer
            .behaviour_mut()
            .list
            .allow_peer(*listener.local_peer_id());
        listener
            .behaviour_mut()
            .list
            .allow_peer(*dialer.local_peer_id());

        dialer.connect(&mut listener).await;

        dialer
            .behaviour_mut()
            .list
            .disallow_peer(*listener.local_peer_id());
        let (
            [SwarmEvent::ConnectionClosed {
                peer_id: closed_dialer_peer,
                ..
            }],
            [SwarmEvent::ConnectionClosed {
                peer_id: closed_listener_peer,
                ..
            }],
        ) = libp2p_swarm_test::drive(&mut dialer, &mut listener).await
        else {
            panic!("unexpected events")
        };
        assert_eq!(closed_dialer_peer, *listener.local_peer_id());
        assert_eq!(closed_listener_peer, *dialer.local_peer_id());
    }

    fn dial<S>(
        dialer: &mut Swarm<Behaviour<S>>,
        listener: &Swarm<Behaviour<S>>,
    ) -> Result<(), DialError>
    where
        S: Enforce,
    {
        dialer.dial(
            DialOpts::peer_id(*listener.local_peer_id())
                .addresses(listener.external_addresses().cloned().collect())
                .build(),
        )
    }

    #[derive(libp2p_swarm_derive::NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Behaviour<S> {
        list: super::Behaviour<S>,
        keep_alive: libp2p_swarm::keep_alive::Behaviour,
    }

    impl<S> Behaviour<S>
    where
        S: Default,
    {
        fn new() -> Self {
            Self {
                list: super::Behaviour {
                    waker: None,
                    close_connections: VecDeque::new(),
                    state: S::default(),
                },
                keep_alive: libp2p_swarm::keep_alive::Behaviour,
            }
        }
    }
}
