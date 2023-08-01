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

use super::*;

/// The configurable static connection limits.
#[derive(Debug, Clone, Default)]
pub struct StaticConnectionLimits {
    pub(crate) max_pending_incoming: Option<u32>,
    pub(crate) max_pending_outgoing: Option<u32>,
    pub(crate) max_established_incoming: Option<u32>,
    pub(crate) max_established_outgoing: Option<u32>,
    pub(crate) max_established_per_peer: Option<u32>,
    pub(crate) max_established_total: Option<u32>,
}

impl StaticConnectionLimits {
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

impl ConnectionLimitsChecker for StaticConnectionLimits {
    fn check_limit(&self, kind: ConnectionKind, current: usize) -> Result<(), ConnectionDenied> {
        use ConnectionKind::*;

        let limit = match kind {
            PendingIncoming => self.max_pending_incoming,
            PendingOutgoing => self.max_pending_outgoing,
            EstablishedIncoming => self.max_established_incoming,
            EstablishedOutgoing => self.max_established_outgoing,
            EstablishedPerPeer => self.max_established_per_peer,
            EstablishedTotal => self.max_established_total,
        }
        .unwrap_or(u32::MAX);

        if current as u32 >= limit {
            return Err(ConnectionDenied::new(StaticLimitExceeded { limit, kind }));
        }

        Ok(())
    }
}

/// A connection limit has been exceeded.
#[derive(Debug, Clone, Copy)]
pub struct StaticLimitExceeded {
    limit: u32,
    kind: ConnectionKind,
}

impl StaticLimitExceeded {
    pub fn limit(&self) -> u32 {
        self.limit
    }
}

impl std::error::Error for StaticLimitExceeded {}

impl fmt::Display for StaticLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "connection limit exceeded: at most {} {} are allowed",
            self.limit, self.kind
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use libp2p_swarm::{
        behaviour::toggle::Toggle, dial_opts::DialOpts, DialError, ListenError, Swarm, SwarmEvent,
    };
    use libp2p_swarm_test::SwarmExt;
    use quickcheck::*;

    #[test]
    fn max_outgoing() {
        use rand::Rng;

        let outgoing_limit = rand::thread_rng().gen_range(1..10);

        let mut network = Swarm::new_ephemeral(|_| {
            Behaviour::new(
                StaticConnectionLimits::default().with_max_pending_outgoing(Some(outgoing_limit)),
            )
        });

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
                let exceeded = cause
                    .downcast::<StaticLimitExceeded>()
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
                    StaticConnectionLimits::default().with_max_established_incoming(Some(limit)),
                )
            });
            let mut swarm2 = Swarm::new_ephemeral(|_| {
                Behaviour::new(
                    StaticConnectionLimits::default().with_max_established_incoming(Some(limit)),
                )
            });

            async_std::task::block_on(async {
                let (listen_addr, _) = swarm1.listen().await;

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

                assert_eq!(
                    cause.downcast::<StaticLimitExceeded>().unwrap().limit,
                    limit
                );
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
            Behaviour::new_with_connection_denier(StaticConnectionLimits::default())
        });
        let mut swarm2 =
            Swarm::new_ephemeral(|_| Behaviour::new(StaticConnectionLimits::default()));

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
        limits: crate::Behaviour<StaticConnectionLimits>,
        keep_alive: libp2p_swarm::keep_alive::Behaviour,
        connection_denier: Toggle<ConnectionDenier>,
    }

    impl Behaviour {
        fn new(limits: StaticConnectionLimits) -> Self {
            Self {
                limits: crate::Behaviour::new(limits),
                keep_alive: libp2p_swarm::keep_alive::Behaviour,
                connection_denier: None.into(),
            }
        }
        fn new_with_connection_denier(limits: StaticConnectionLimits) -> Self {
            Self {
                limits: crate::Behaviour::new(limits),
                keep_alive: libp2p_swarm::keep_alive::Behaviour,
                connection_denier: Some(ConnectionDenier {}).into(),
            }
        }
    }

    struct ConnectionDenier {}

    impl NetworkBehaviour for ConnectionDenier {
        type ConnectionHandler = dummy::ConnectionHandler;
        type ToSwarm = Void;

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
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Err(ConnectionDenied::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "ConnectionDenier",
            )))
        }

        fn on_swarm_event(&mut self, _event: FromSwarm<Self::ConnectionHandler>) {}

        fn on_connection_handler_event(
            &mut self,
            _peer_id: PeerId,
            _connection_id: ConnectionId,
            event: THandlerOutEvent<Self>,
        ) {
            void::unreachable(event)
        }

        fn poll(
            &mut self,
            _cx: &mut Context<'_>,
            _params: &mut impl PollParameters,
        ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }
    }
}
