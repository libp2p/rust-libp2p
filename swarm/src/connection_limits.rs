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

use crate::{
    dummy, ConnectionClosed, ConnectionDenied, ConnectionId, ConnectionLimit, ConnectionLimits,
    FromSwarm, NetworkBehaviour, NetworkBehaviourAction, PollParameters, THandler, THandlerInEvent,
    THandlerOutEvent,
};
use libp2p_core::{Endpoint, Multiaddr, PeerId};
use std::collections::{HashMap, HashSet};
use std::task::{Context, Poll};
use void::Void;

/// A [`NetworkBehaviour`] that enforces a set of [`ConnectionLimits`].
///
/// For these limits to take effect, this needs to be composed into the behaviour tree of your application.
///
/// If a connection is denied due to a limit, either a [`SwarmEvent::IncomingConnectionError`](crate::SwarmEvent::IncomingConnectionError)
/// or [`SwarmEvent::OutgoingConnectionError`] will be emitted.
/// The [`ListenError::Denied`](crate::ListenError::Denied) and respectively the [`DialError::Denied`](crate::DialError::Denied) variant
/// contain a [`ConnectionDenied`](crate::ConnectionDenied) type that can be downcast to [`ConnectionLimit`] error if (and only if) **this**
/// behaviour denied the connection.
///
/// If you employ multiple [`NetworkBehaviour`]s that manage connections, it may also be a different error.
///
/// # Example
///
/// ```rust
/// # use libp2p_identify as identify;
/// # use libp2p_ping as ping;
/// # use libp2p_swarm_derive::NetworkBehaviour;
///
/// #[derive(NetworkBehaviour)]
/// # #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// struct MyBehaviour {
///   identify: identify::Behaviour,
///   ping: ping::Behaviour,
///   limits: connection_limits::Behaviour
/// }
/// ```
pub struct Behaviour {
    limits: ConnectionLimits,

    pending_inbound_connections: HashSet<ConnectionId>,
    pending_outbound_connections: HashSet<ConnectionId>,
    established_inbound_connections: HashSet<ConnectionId>,
    established_outbound_connections: HashSet<ConnectionId>,
    established_per_peer: HashMap<PeerId, HashSet<ConnectionId>>,
}

impl Behaviour {
    pub fn new(limits: ConnectionLimits) -> Self {
        Self {
            limits,
            pending_inbound_connections: Default::default(),
            pending_outbound_connections: Default::default(),
            established_inbound_connections: Default::default(),
            established_outbound_connections: Default::default(),
            established_per_peer: Default::default(),
        }
    }

    fn check_limit(&mut self, limit: Option<u32>, current: usize) -> Result<(), ConnectionDenied> {
        let limit = limit.unwrap_or(u32::MAX);
        let current = current as u32;

        if current >= limit {
            return Err(ConnectionDenied::new(ConnectionLimit { limit, current }));
        }

        Ok(())
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type OutEvent = Void;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.check_limit(
            self.limits.max_pending_incoming,
            self.pending_inbound_connections.len(),
        )?;

        self.pending_inbound_connections.insert(connection_id);

        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_inbound_connections.remove(&connection_id);

        self.check_limit(
            self.limits.max_established_incoming,
            self.established_inbound_connections.len(),
        )?;
        self.check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
        )?;
        self.check_limit(
            self.limits.max_established_total,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
        )?;

        self.established_inbound_connections.insert(connection_id);
        self.established_per_peer
            .entry(peer)
            .or_default()
            .insert(connection_id);

        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.check_limit(
            self.limits.max_pending_outgoing,
            self.pending_outbound_connections.len(),
        )?;

        self.pending_outbound_connections.insert(connection_id);

        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_outbound_connections.remove(&connection_id);

        self.check_limit(
            self.limits.max_established_outgoing,
            self.established_outbound_connections.len(),
        )?;
        self.check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
        )?;
        self.check_limit(
            self.limits.max_established_total,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
        )?;

        self.established_outbound_connections.insert(connection_id);
        self.established_per_peer
            .entry(peer)
            .or_default()
            .insert(connection_id);

        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                self.established_inbound_connections.remove(&connection_id);
                self.established_outbound_connections.remove(&connection_id);
                self.established_per_peer
                    .entry(peer_id)
                    .or_default()
                    .remove(&connection_id);
            }
            FromSwarm::ConnectionEstablished(_) => {}
            FromSwarm::AddressChange(_) => {}
            FromSwarm::DialFailure(_) => {}
            FromSwarm::ListenFailure(_) => {}
            FromSwarm::NewListener(_) => {}
            FromSwarm::NewListenAddr(_) => {}
            FromSwarm::ExpiredListenAddr(_) => {}
            FromSwarm::ListenerError(_) => {}
            FromSwarm::ListenerClosed(_) => {}
            FromSwarm::NewExternalAddr(_) => {}
            FromSwarm::ExpiredExternalAddr(_) => {}
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
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DialError, DialOpts, ListenError, Swarm, SwarmEvent};
    use futures::future;
    use futures::ready;
    use futures::StreamExt;
    use libp2p_core::{identity, multiaddr::multiaddr, transport, upgrade, Transport};
    use libp2p_plaintext as plaintext;
    use libp2p_yamux as yamux;
    use quickcheck::*;

    #[test]
    fn max_outgoing() {
        use rand::Rng;

        let outgoing_limit = rand::thread_rng().gen_range(1..10);

        let mut network =
            new_swarm(ConnectionLimits::default().with_max_pending_outgoing(Some(outgoing_limit)));

        let addr: Multiaddr = "/memory/1234".parse().unwrap();

        let target = PeerId::random();
        for _ in 0..outgoing_limit {
            network
                .dial(
                    DialOpts::peer_id(target)
                        .addresses(vec![addr.clone()])
                        .build(),
                )
                .expect("Unexpected connection limit.");
        }

        match network
            .dial(DialOpts::peer_id(target).addresses(vec![addr]).build())
            .expect_err("Unexpected dialing success.")
        {
            DialError::Denied { cause } => {
                let limit = cause
                    .downcast::<ConnectionLimit>()
                    .expect("connection denied because of limit");

                assert_eq!(limit.current, outgoing_limit);
                assert_eq!(limit.limit, outgoing_limit);
            }
            e => panic!("Unexpected error: {e:?}"),
        }

        let info = network.network_info();
        assert_eq!(info.num_peers(), 0);
        assert_eq!(
            info.connection_counters().num_pending_outgoing(),
            outgoing_limit
        );
    }

    #[test]
    fn max_established_incoming() {
        #[derive(Debug, Clone)]
        struct Limit(u32);

        impl Arbitrary for Limit {
            fn arbitrary(g: &mut Gen) -> Self {
                Self(g.gen_range(1..10))
            }
        }

        fn limits(limit: u32) -> ConnectionLimits {
            ConnectionLimits::default().with_max_established_incoming(Some(limit))
        }

        fn prop(limit: Limit) {
            let limit = limit.0;

            let mut network1 = new_swarm(limits(limit));
            let mut network2 = new_swarm(limits(limit));

            let _ = network1.listen_on(multiaddr![Memory(0u64)]).unwrap();
            let listen_addr = async_std::task::block_on(future::poll_fn(|cx| {
                match ready!(network1.poll_next_unpin(cx)).unwrap() {
                    SwarmEvent::NewListenAddr { address, .. } => Poll::Ready(address),
                    e => panic!("Unexpected network event: {e:?}"),
                }
            }));

            // Spawn and block on the dialer.
            async_std::task::block_on({
                let mut n = 0;
                network2.dial(listen_addr.clone()).unwrap();

                let mut expected_closed = false;
                let mut network_1_established = false;
                let mut network_2_established = false;
                let mut network_1_limit_reached = false;
                let mut network_2_limit_reached = false;
                future::poll_fn(move |cx| {
                    loop {
                        let mut network_1_pending = false;
                        let mut network_2_pending = false;

                        match network1.poll_next_unpin(cx) {
                            Poll::Ready(Some(SwarmEvent::IncomingConnection { .. })) => {}
                            Poll::Ready(Some(SwarmEvent::ConnectionEstablished { .. })) => {
                                network_1_established = true;
                            }
                            Poll::Ready(Some(SwarmEvent::IncomingConnectionError {
                                error: ListenError::Denied { cause },
                                ..
                            })) => {
                                let err = cause
                                    .downcast::<ConnectionLimit>()
                                    .expect("connection denied because of limit");

                                assert_eq!(err.limit, limit);
                                assert_eq!(err.limit, err.current);
                                let info = network1.network_info();
                                let counters = info.connection_counters();
                                assert_eq!(counters.num_established_incoming(), limit);
                                assert_eq!(counters.num_established(), limit);
                                network_1_limit_reached = true;
                            }
                            Poll::Pending => {
                                network_1_pending = true;
                            }
                            e => panic!("Unexpected network event: {e:?}"),
                        }

                        match network2.poll_next_unpin(cx) {
                            Poll::Ready(Some(SwarmEvent::ConnectionEstablished { .. })) => {
                                network_2_established = true;
                            }
                            Poll::Ready(Some(SwarmEvent::ConnectionClosed { .. })) => {
                                assert!(expected_closed);
                                let info = network2.network_info();
                                let counters = info.connection_counters();
                                assert_eq!(counters.num_established_outgoing(), limit);
                                assert_eq!(counters.num_established(), limit);
                                network_2_limit_reached = true;
                            }
                            Poll::Pending => {
                                network_2_pending = true;
                            }
                            e => panic!("Unexpected network event: {e:?}"),
                        }

                        if network_1_pending && network_2_pending {
                            return Poll::Pending;
                        }

                        if network_1_established && network_2_established {
                            network_1_established = false;
                            network_2_established = false;

                            if n <= limit {
                                // Dial again until the limit is exceeded.
                                n += 1;
                                network2.dial(listen_addr.clone()).unwrap();

                                if n == limit {
                                    // The the next dialing attempt exceeds the limit, this
                                    // is the connection we expected to get closed.
                                    expected_closed = true;
                                }
                            } else {
                                panic!("Expect networks not to establish connections beyond the limit.")
                            }
                        }

                        if network_1_limit_reached && network_2_limit_reached {
                            return Poll::Ready(());
                        }
                    }
                })
            });
        }

        quickcheck(prop as fn(_));
    }

    fn new_swarm(limits: ConnectionLimits) -> Swarm<Behaviour> {
        let id_keys = identity::Keypair::generate_ed25519();
        let local_public_key = id_keys.public();
        let transport = transport::MemoryTransport::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(plaintext::PlainText2Config {
                local_public_key: local_public_key.clone(),
            })
            .multiplex(yamux::YamuxConfig::default())
            .boxed();
        let behaviour = Behaviour {
            limits: super::Behaviour::new(limits),
            keep_alive: crate::keep_alive::Behaviour,
        };

        Swarm::without_executor(transport, behaviour, local_public_key.to_peer_id())
    }

    #[derive(libp2p_swarm_derive::NetworkBehaviour)]
    #[behaviour(prelude = "crate::derive_prelude")]
    struct Behaviour {
        limits: crate::connection_limits::Behaviour,
        keep_alive: crate::keep_alive::Behaviour,
    }
}
