use async_trait::async_trait;
use futures::future::Either;
use futures::StreamExt;
use libp2p_core::{
    identity::Keypair, multiaddr::Protocol, transport::MemoryTransport, upgrade::Version,
    Multiaddr, PeerId, Transport,
};
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::{
    dial_opts::DialOpts, AddressScore, NetworkBehaviour, Swarm, SwarmEvent, THandlerErr,
};
use libp2p_yamux::YamuxConfig;
use std::fmt::Debug;
use std::time::Duration;

/// An extension trait for [`Swarm`] that makes it easier to set up a network of [`Swarm`]s for tests.
#[async_trait]
pub trait SwarmExt {
    type NB: NetworkBehaviour;

    /// Create a new [`Swarm`] with an ephemeral identity.
    ///
    /// The swarm will use a [`MemoryTransport`] together with a noise authentication layer and
    /// yamux as the multiplexer. However, these details should not be relied upon by the test
    /// and may change at any time.
    fn new_ephemeral(behaviour_fn: impl FnOnce(Keypair) -> Self::NB) -> Self
    where
        Self: Sized;

    /// Establishes a connection to the given [`Swarm`], polling both of them until the connection is established.
    async fn connect<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::OutEvent: Debug;

    /// Dial the provided address and wait until a connection has been established.
    ///
    /// In a normal test scenario, you should prefer [`SwarmExt::connect`] but that is not always possible.
    /// This function only abstracts away the "dial and wait for `ConnectionEstablished` event" part.
    ///
    /// Because we don't have access to the other [`Swarm`], we can't guarantee that it makes progress.
    async fn dial_and_wait(&mut self, addr: Multiaddr) -> PeerId;

    /// Wait for specified condition to return `Some`.
    async fn wait<E, P>(&mut self, predicate: P) -> E
    where
        P: Fn(
            SwarmEvent<<Self::NB as NetworkBehaviour>::OutEvent, THandlerErr<Self::NB>>,
        ) -> Option<E>,
        P: Send;

    /// Listens for incoming connections, polling the [`Swarm`] until the transport is ready to accept connections.
    ///
    /// The first address is for the memory transport, the second one for the TCP transport.
    async fn listen(&mut self) -> (Multiaddr, Multiaddr);

    /// Returns the next [`SwarmEvent`] or times out after 10 seconds.
    ///
    /// If the 10s timeout does not fit your usecase, please fall back to `StreamExt::next`.
    async fn next_or_timeout(
        &mut self,
    ) -> SwarmEvent<<Self::NB as NetworkBehaviour>::OutEvent, THandlerErr<Self::NB>>;

    /// Returns the next behaviour event or times out after 10 seconds.
    ///
    /// If the 10s timeout does not fit your usecase, please fall back to `StreamExt::next`.
    async fn next_behaviour_event(&mut self) -> <Self::NB as NetworkBehaviour>::OutEvent;

    async fn loop_on_next(self);
}

#[async_trait]
impl<B> SwarmExt for Swarm<B>
where
    B: NetworkBehaviour + Send,
    <B as NetworkBehaviour>::OutEvent: Debug,
{
    type NB = B;

    fn new_ephemeral(behaviour_fn: impl FnOnce(Keypair) -> Self::NB) -> Self
    where
        Self: Sized,
    {
        let identity = Keypair::generate_ed25519();
        let peer_id = PeerId::from(identity.public());

        let transport = MemoryTransport::default()
            .or_transport(libp2p_tcp::async_io::Transport::default())
            .upgrade(Version::V1)
            .authenticate(PlainText2Config {
                local_public_key: identity.public(),
            })
            .multiplex(YamuxConfig::default())
            .timeout(Duration::from_secs(20))
            .boxed();

        Swarm::without_executor(transport, behaviour_fn(identity), peer_id)
    }

    async fn connect<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
        let external_addresses = other
            .external_addresses()
            .cloned()
            .map(|r| r.addr)
            .collect();

        let dial_opts = DialOpts::peer_id(*other.local_peer_id())
            .addresses(external_addresses)
            .build();

        self.dial(dial_opts).unwrap();

        let mut dialer_done = false;
        let mut listener_done = false;

        loop {
            match futures::future::select(self.next_or_timeout(), other.next_or_timeout()).await {
                Either::Left((SwarmEvent::ConnectionEstablished { .. }, _)) => {
                    dialer_done = true;
                }
                Either::Right((SwarmEvent::ConnectionEstablished { .. }, _)) => {
                    listener_done = true;
                }
                Either::Left((other, _)) => {
                    log::debug!("Ignoring event from dialer {:?}", other);
                }
                Either::Right((other, _)) => {
                    log::debug!("Ignoring event from listener {:?}", other);
                }
            }

            if dialer_done && listener_done {
                return;
            }
        }
    }

    async fn dial_and_wait(&mut self, addr: Multiaddr) -> PeerId {
        self.dial(addr.clone()).unwrap();

        self.wait(|e| match e {
            SwarmEvent::ConnectionEstablished {
                endpoint, peer_id, ..
            } => (endpoint.get_remote_address() == &addr).then_some(peer_id),
            other => {
                log::debug!("Ignoring event from dialer {:?}", other);
                None
            }
        })
        .await
    }

    async fn wait<E, P>(&mut self, predicate: P) -> E
    where
        P: Fn(SwarmEvent<<B as NetworkBehaviour>::OutEvent, THandlerErr<B>>) -> Option<E>,
        P: Send,
    {
        loop {
            let event = self.next_or_timeout().await;
            if let Some(e) = predicate(event) {
                break e;
            }
        }
    }

    async fn listen(&mut self) -> (Multiaddr, Multiaddr) {
        let memory_addr_listener_id = self.listen_on(Protocol::Memory(0).into()).unwrap();
        let tcp_addr_listener_id = self
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .unwrap();

        // block until we are actually listening
        let memory_multiaddr = self
            .wait(|e| match e {
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } => (listener_id == memory_addr_listener_id).then_some(address),
                other => {
                    log::debug!(
                        "Ignoring {:?} while waiting for listening to succeed",
                        other
                    );
                    None
                }
            })
            .await;

        // Memory addresses are externally reachable because they all share the same memory-space.
        self.add_external_address(memory_multiaddr.clone(), AddressScore::Infinite);

        let tcp_multiaddr = self
            .wait(|e| match e {
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } => (listener_id == tcp_addr_listener_id).then_some(address),
                other => {
                    log::debug!(
                        "Ignoring {:?} while waiting for listening to succeed",
                        other
                    );
                    None
                }
            })
            .await;

        // TCP addresses are "externally" reachable because we run our tests in the same process and they all can reach localhost.
        self.add_external_address(tcp_multiaddr.clone(), AddressScore::Infinite);

        (memory_multiaddr, tcp_multiaddr)
    }

    async fn next_or_timeout(
        &mut self,
    ) -> SwarmEvent<<Self::NB as NetworkBehaviour>::OutEvent, THandlerErr<Self::NB>> {
        match futures::future::select(
            futures_timer::Delay::new(Duration::from_secs(10)),
            self.select_next_some(),
        )
        .await
        {
            Either::Left(((), _)) => panic!("Swarm did not emit an event within 10s"),
            Either::Right((event, _)) => {
                log::trace!("Swarm produced: {:?}", event);

                event
            }
        }
    }

    async fn next_behaviour_event(&mut self) -> <Self::NB as NetworkBehaviour>::OutEvent {
        loop {
            if let Ok(event) = self.next_or_timeout().await.try_into_behaviour_event() {
                return event;
            }
        }
    }

    async fn loop_on_next(mut self) {
        while let Some(event) = self.next().await {
            log::debug!("Swarm produced: {:?}", event);
        }
    }
}
