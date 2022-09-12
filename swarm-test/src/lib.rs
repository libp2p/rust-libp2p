use async_trait::async_trait;
use futures::future::Either;
use futures::StreamExt;
use libp2p::swarm::{
    AddressScore, ConnectionHandler, IntoConnectionHandler, NetworkBehaviour, SwarmEvent,
};
use libp2p::{Multiaddr, Swarm};
use std::fmt::Debug;
use std::time::Duration;

type THandler<TBehaviour> = <TBehaviour as NetworkBehaviour>::ConnectionHandler;
type THandlerErr<TBehaviour> =
    <<THandler<TBehaviour> as IntoConnectionHandler>::Handler as ConnectionHandler>::Error;

/// An extension trait for [`Swarm`] that makes it easier to set up a network of [`Swarm`]s for tests.
#[async_trait]
pub trait SwarmExt {
    type NB: NetworkBehaviour;

    /// Establishes a connection to the given [`Swarm`], polling both of them until the connection is established.
    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::OutEvent: Debug;

    /// Listens on a random memory address, polling the [`Swarm`] until the transport is ready to accept connections.
    async fn listen_on_random_memory_address(&mut self) -> Multiaddr;

    async fn next_within(
        &mut self,
        seconds: u64,
    ) -> SwarmEvent<<Self::NB as NetworkBehaviour>::OutEvent, THandlerErr<Self::NB>>;

    async fn loop_on_next(self);
}

#[async_trait]
impl<B> SwarmExt for Swarm<B>
where
    B: NetworkBehaviour + Send,
    <B as NetworkBehaviour>::OutEvent: Debug,
{
    type NB = B;

    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
        let addr_to_dial = other.external_addresses().next().unwrap().addr.clone();

        self.dial(addr_to_dial.clone()).unwrap();

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
        let memory_addr_listener_id = self.listen_on(get_rand_memory_address()).unwrap();

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

    async fn next_within(
        &mut self,
        seconds: u64,
    ) -> SwarmEvent<<Self::NB as NetworkBehaviour>::OutEvent, THandlerErr<Self::NB>> {
        match futures::future::select(
            futures_timer::Delay::new(Duration::from_secs(seconds)),
            self.select_next_some(),
        )
        .await
        {
            Either::Left(((), _)) => panic!("Swarm did not emit an event within {seconds}s"),
            Either::Right((event, _)) => {
                log::trace!("Swarm produced: {:?}", event);

                event
            }
        }
    }

    async fn loop_on_next(mut self) {
        while let Some(event) = self.next().await {
            log::debug!("Swarm produced: {:?}", event);
        }
    }
}

fn get_rand_memory_address() -> Multiaddr {
    let address_port = rand::random::<u64>();
    let addr = format!("/memory/{}", address_port)
        .parse::<Multiaddr>()
        .unwrap();

    addr
}
