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

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    task::{Context, Poll},
};

use libp2p_core::{transport::PortUse, ConnectedPoint, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionEstablished, DialFailure, ListenFailure},
    dummy, ConnectionClosed, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};

/// A [`NetworkBehaviour`] that enforces a set of [`ConnectionLimits`].
///
/// For these limits to take effect, this needs to be composed
/// into the behaviour tree of your application.
///
/// If a connection is denied due to a limit, either a
/// [`SwarmEvent::IncomingConnectionError`](libp2p_swarm::SwarmEvent::IncomingConnectionError)
/// or [`SwarmEvent::OutgoingConnectionError`](libp2p_swarm::SwarmEvent::OutgoingConnectionError)
/// will be emitted. The [`ListenError::Denied`](libp2p_swarm::ListenError::Denied) and respectively
/// the [`DialError::Denied`](libp2p_swarm::DialError::Denied) variant
/// contain a [`ConnectionDenied`] type that can be downcast to [`Exceeded`] error if (and only if)
/// **this** behaviour denied the connection.
///
/// If you employ multiple [`NetworkBehaviour`]s that manage connections,
/// it may also be a different error.
///
/// # Example
///
/// ```rust
/// # use libp2p_identify as identify;
/// # use libp2p_ping as ping;
/// # use libp2p_swarm_derive::NetworkBehaviour;
/// # use libp2p_connection_limits as connection_limits;
///
/// #[derive(NetworkBehaviour)]
/// # #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// struct MyBehaviour {
///     identify: identify::Behaviour,
///     ping: ping::Behaviour,
///     limits: connection_limits::Behaviour,
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

    /// Returns a mutable reference to [`ConnectionLimits`].
    /// > **Note**: A new limit will not be enforced against existing connections.
    pub fn limits_mut(&mut self) -> &mut ConnectionLimits {
        &mut self.limits
    }
}

fn check_limit(limit: Option<u32>, current: usize, kind: Kind) -> Result<(), ConnectionDenied> {
    let limit = limit.unwrap_or(u32::MAX);
    let current = current as u32;

    if current >= limit {
        return Err(ConnectionDenied::new(Exceeded { limit, kind }));
    }

    Ok(())
}

/// A connection limit has been exceeded.
#[derive(Debug, Clone, Copy)]
pub struct Exceeded {
    limit: u32,
    kind: Kind,
}

impl Exceeded {
    pub fn limit(&self) -> u32 {
        self.limit
    }
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "connection limit exceeded: at most {} {} are allowed",
            self.limit, self.kind
        )
    }
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    PendingIncoming,
    PendingOutgoing,
    EstablishedIncoming,
    EstablishedOutgoing,
    EstablishedPerPeer,
    EstablishedTotal,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::PendingIncoming => write!(f, "pending incoming connections"),
            Kind::PendingOutgoing => write!(f, "pending outgoing connections"),
            Kind::EstablishedIncoming => write!(f, "established incoming connections"),
            Kind::EstablishedOutgoing => write!(f, "established outgoing connections"),
            Kind::EstablishedPerPeer => write!(f, "established connections per peer"),
            Kind::EstablishedTotal => write!(f, "established connections"),
        }
    }
}

impl std::error::Error for Exceeded {}

/// The configurable connection limits.
#[derive(Debug, Clone, Default)]
pub struct ConnectionLimits {
    max_pending_incoming: Option<u32>,
    max_pending_outgoing: Option<u32>,
    max_established_incoming: Option<u32>,
    max_established_outgoing: Option<u32>,
    max_established_per_peer: Option<u32>,
    max_established_total: Option<u32>,
}

impl ConnectionLimits {
    /// Configures the maximum number of concurrently incoming connections being established.
    pub fn with_max_pending_incoming(mut self, limit: Option<u32>) -> Self {
        self.max_pending_incoming = limit;
        self
    }

    /// Configures the maximum number of concurrently outgoing connections being established.
    pub fn with_max_pending_outgoing(mut self, limit: Option<u32>) -> Self {
        self.max_pending_outgoing = limit;
        self
    }

    /// Configures the maximum number of concurrent established inbound connections.
    pub fn with_max_established_incoming(mut self, limit: Option<u32>) -> Self {
        self.max_established_incoming = limit;
        self
    }

    /// Configures the maximum number of concurrent established outbound connections.
    pub fn with_max_established_outgoing(mut self, limit: Option<u32>) -> Self {
        self.max_established_outgoing = limit;
        self
    }

    /// Configures the maximum number of concurrent established connections (both
    /// inbound and outbound).
    ///
    /// Note: This should be used in conjunction with
    /// [`ConnectionLimits::with_max_established_incoming`] to prevent possible
    /// eclipse attacks (all connections being inbound).
    pub fn with_max_established(mut self, limit: Option<u32>) -> Self {
        self.max_established_total = limit;
        self
    }

    /// Configures the maximum number of concurrent established connections per peer,
    /// regardless of direction (incoming or outgoing).
    pub fn with_max_established_per_peer(mut self, limit: Option<u32>) -> Self {
        self.max_established_per_peer = limit;
        self
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        check_limit(
            self.limits.max_pending_incoming,
            self.pending_inbound_connections.len(),
            Kind::PendingIncoming,
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

        check_limit(
            self.limits.max_established_incoming,
            self.established_inbound_connections.len(),
            Kind::EstablishedIncoming,
        )?;
        check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
            Kind::EstablishedPerPeer,
        )?;
        check_limit(
            self.limits.max_established_total,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
            Kind::EstablishedTotal,
        )?;

        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        check_limit(
            self.limits.max_pending_outgoing,
            self.pending_outbound_connections.len(),
            Kind::PendingOutgoing,
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
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_outbound_connections.remove(&connection_id);

        check_limit(
            self.limits.max_established_outgoing,
            self.established_outbound_connections.len(),
            Kind::EstablishedOutgoing,
        )?;
        check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
            Kind::EstablishedPerPeer,
        )?;
        check_limit(
            self.limits.max_established_total,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
            Kind::EstablishedTotal,
        )?;

        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
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
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                endpoint,
                connection_id,
                ..
            }) => {
                match endpoint {
                    ConnectedPoint::Listener { .. } => {
                        self.established_inbound_connections.insert(connection_id);
                    }
                    ConnectedPoint::Dialer { .. } => {
                        self.established_outbound_connections.insert(connection_id);
                    }
                }

                self.established_per_peer
                    .entry(peer_id)
                    .or_default()
                    .insert(connection_id);
            }
            FromSwarm::DialFailure(DialFailure { connection_id, .. }) => {
                self.pending_outbound_connections.remove(&connection_id);
            }
            FromSwarm::ListenFailure(ListenFailure { connection_id, .. }) => {
                self.pending_inbound_connections.remove(&connection_id);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        // TODO: remove when Rust 1.82 is MSRV
        #[allow(unreachable_patterns)]
        libp2p_core::util::unreachable(event)
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use libp2p_swarm::{
        behaviour::toggle::Toggle,
        dial_opts::{DialOpts, PeerCondition},
        DialError, ListenError, Swarm, SwarmEvent,
    };
    use libp2p_swarm_test::SwarmExt;
    use quickcheck::*;

    use super::*;

    #[test]
    fn max_outgoing() {
        use rand::Rng;

        let outgoing_limit = rand::thread_rng().gen_range(1..10);

        let mut network = Swarm::new_ephemeral(|_| {
            Behaviour::new(
                ConnectionLimits::default().with_max_pending_outgoing(Some(outgoing_limit)),
            )
        });

        let addr: Multiaddr = "/memory/1234".parse().unwrap();
        let target = PeerId::random();

        for _ in 0..outgoing_limit {
            network
                .dial(
                    DialOpts::peer_id(target)
                        // Dial always, even if already dialing or connected.
                        .condition(PeerCondition::Always)
                        .addresses(vec![addr.clone()])
                        .build(),
                )
                .expect("Unexpected connection limit.");
        }

        match network
            .dial(
                DialOpts::peer_id(target)
                    .condition(PeerCondition::Always)
                    .addresses(vec![addr])
                    .build(),
            )
            .expect_err("Unexpected dialing success.")
        {
            DialError::Denied { cause } => {
                let exceeded = cause
                    .downcast::<Exceeded>()
                    .expect("connection denied because of limit");

                assert_eq!(exceeded.limit(), outgoing_limit);
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
        fn prop(Limit(limit): Limit) {
            let mut swarm1 = Swarm::new_ephemeral(|_| {
                Behaviour::new(
                    ConnectionLimits::default().with_max_established_incoming(Some(limit)),
                )
            });
            let mut swarm2 = Swarm::new_ephemeral(|_| {
                Behaviour::new(
                    ConnectionLimits::default().with_max_established_incoming(Some(limit)),
                )
            });

            async_std::task::block_on(async {
                let (listen_addr, _) = swarm1.listen().with_memory_addr_external().await;

                for _ in 0..limit {
                    swarm2.connect(&mut swarm1).await;
                }

                swarm2.dial(listen_addr).unwrap();

                async_std::task::spawn(swarm2.loop_on_next());

                let cause = swarm1
                    .wait(|event| match event {
                        SwarmEvent::IncomingConnectionError {
                            error: ListenError::Denied { cause },
                            ..
                        } => Some(cause),
                        _ => None,
                    })
                    .await;

                assert_eq!(cause.downcast::<Exceeded>().unwrap().limit, limit);
            });
        }

        #[derive(Debug, Clone)]
        struct Limit(u32);

        impl Arbitrary for Limit {
            fn arbitrary(g: &mut Gen) -> Self {
                Self(g.gen_range(1..10))
            }
        }

        quickcheck(prop as fn(_));
    }

    /// Another sibling [`NetworkBehaviour`] implementation might deny established connections in
    /// [`handle_established_outbound_connection`] or [`handle_established_inbound_connection`].
    /// [`Behaviour`] must not increase the established counters in
    /// [`handle_established_outbound_connection`] or [`handle_established_inbound_connection`], but
    /// in [`SwarmEvent::ConnectionEstablished`] as the connection might still be denied by a
    /// sibling [`NetworkBehaviour`] in the former case. Only in the latter case
    /// ([`SwarmEvent::ConnectionEstablished`]) can the connection be seen as established.
    #[test]
    fn support_other_behaviour_denying_connection() {
        let mut swarm1 = Swarm::new_ephemeral(|_| {
            Behaviour::new_with_connection_denier(ConnectionLimits::default())
        });
        let mut swarm2 = Swarm::new_ephemeral(|_| Behaviour::new(ConnectionLimits::default()));

        async_std::task::block_on(async {
            // Have swarm2 dial swarm1.
            let (listen_addr, _) = swarm1.listen().await;
            swarm2.dial(listen_addr).unwrap();
            async_std::task::spawn(swarm2.loop_on_next());

            // Wait for the ConnectionDenier of swarm1 to deny the established connection.
            let cause = swarm1
                .wait(|event| match event {
                    SwarmEvent::IncomingConnectionError {
                        error: ListenError::Denied { cause },
                        ..
                    } => Some(cause),
                    _ => None,
                })
                .await;

            cause.downcast::<std::io::Error>().unwrap();

            assert_eq!(
                0,
                swarm1
                    .behaviour_mut()
                    .limits
                    .established_inbound_connections
                    .len(),
                "swarm1 connection limit behaviour to not count denied established connection as established connection"
            )
        });
    }

    #[derive(libp2p_swarm_derive::NetworkBehaviour)]
    #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
    struct Behaviour {
        limits: super::Behaviour,
        connection_denier: Toggle<ConnectionDenier>,
    }

    impl Behaviour {
        fn new(limits: ConnectionLimits) -> Self {
            Self {
                limits: super::Behaviour::new(limits),
                connection_denier: None.into(),
            }
        }
        fn new_with_connection_denier(limits: ConnectionLimits) -> Self {
            Self {
                limits: super::Behaviour::new(limits),
                connection_denier: Some(ConnectionDenier {}).into(),
            }
        }
    }

    struct ConnectionDenier {}

    impl NetworkBehaviour for ConnectionDenier {
        type ConnectionHandler = dummy::ConnectionHandler;
        type ToSwarm = Infallible;

        fn handle_established_inbound_connection(
            &mut self,
            _connection_id: ConnectionId,
            _peer: PeerId,
            _local_addr: &Multiaddr,
            _remote_addr: &Multiaddr,
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Err(ConnectionDenied::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "ConnectionDenier",
            )))
        }

        fn handle_established_outbound_connection(
            &mut self,
            _connection_id: ConnectionId,
            _peer: PeerId,
            _addr: &Multiaddr,
            _role_override: Endpoint,
            _port_use: PortUse,
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Err(ConnectionDenied::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "ConnectionDenier",
            )))
        }

        fn on_swarm_event(&mut self, _event: FromSwarm) {}

        fn on_connection_handler_event(
            &mut self,
            _peer_id: PeerId,
            _connection_id: ConnectionId,
            event: THandlerOutEvent<Self>,
        ) {
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            libp2p_core::util::unreachable(event)
        }

        fn poll(
            &mut self,
            _: &mut Context<'_>,
        ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }
    }
}
