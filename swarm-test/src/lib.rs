use async_trait::async_trait;
use futures::future::Either;
use futures::StreamExt;
use libp2p::core::transport::MemoryTransport;
use libp2p::core::upgrade::Version;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::noise::NoiseAuthenticated;
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::swarm::{
    AddressScore, ConnectionHandler, IntoConnectionHandler, NetworkBehaviour, SwarmEvent,
};
use libp2p::yamux::YamuxConfig;
use libp2p::{Multiaddr, PeerId, Swarm, Transport};
use std::fmt::Debug;
use std::time::Duration;

type THandler<TBehaviour> = <TBehaviour as NetworkBehaviour>::ConnectionHandler;
type THandlerErr<TBehaviour> =
    <<THandler<TBehaviour> as IntoConnectionHandler>::Handler as ConnectionHandler>::Error;

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

    /// Listens on a random memory address, polling the [`Swarm`] until the transport is ready to accept connections.
    async fn listen_on_random_memory_address(&mut self) -> Multiaddr;

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
            .upgrade(Version::V1)
            .authenticate(NoiseAuthenticated::xx(&identity).unwrap())
            .multiplex(YamuxConfig::default())
            .timeout(Duration::from_secs(20))
            .boxed();

        Swarm::new(transport, behaviour_fn(identity), peer_id)
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
            let mut dialer_event_fut = self.select_next_some();

            futures::select! {
                dialer_event = dialer_event_fut => {
                    match dialer_event {
                        SwarmEvent::ConnectionEstablished { .. } => {
                            dialer_done = true;
                        }
                        other => {
                            log::debug!("Ignoring {:?}", other);
                        }
                    }
                },
                listener_event = other.select_next_some() => {
                    match listener_event {
                        SwarmEvent::ConnectionEstablished { .. } => {
                            listener_done = true;
                        }
                        SwarmEvent::IncomingConnectionError { error, .. } => {
                            panic!("Failure in incoming connection {}", error);
                        }
                        other => {
                            log::debug!("Ignoring {:?}", other);
                        }
                    }
                }
            }

            if dialer_done && listener_done {
                return;
            }
        }
    }

    async fn listen_on_random_memory_address(&mut self) -> Multiaddr {
        let memory_addr_listener_id = self.listen_on(Protocol::Memory(0).into()).unwrap();

        // block until we are actually listening
        let multiaddr = loop {
            match self.select_next_some().await {
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } if listener_id == memory_addr_listener_id => {
                    break address;
                }
                other => {
                    log::debug!(
                        "Ignoring {:?} while waiting for listening to succeed",
                        other
                    );
                }
            }
        };

        // Memory addresses are externally reachable because they all share the same memory-space.
        self.add_external_address(multiaddr.clone(), AddressScore::Infinite);

        multiaddr
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
