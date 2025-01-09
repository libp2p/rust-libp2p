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

use std::{fmt::Debug, future::IntoFuture, time::Duration};

use async_trait::async_trait;
use futures::{
    future::{BoxFuture, Either},
    FutureExt, StreamExt,
};
use libp2p_core::{multiaddr::Protocol, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dial_opts::{DialOpts, PeerCondition},
    NetworkBehaviour, Swarm, SwarmEvent,
};

/// An extension trait for [`Swarm`] that makes it
/// easier to set up a network of [`Swarm`]s for tests.
#[async_trait]
pub trait SwarmExt {
    type NB: NetworkBehaviour;

    /// Create a new [`Swarm`] with an ephemeral identity and the `async-std` runtime.
    ///
    /// The swarm will use a [`libp2p_core::transport::MemoryTransport`] together with a
    /// [`libp2p_plaintext::Config`] authentication layer and [`libp2p_yamux::Config`] as the
    /// multiplexer. However, these details should not be relied
    /// upon by the test and may change at any time.
    #[cfg(feature = "async-std")]
    fn new_ephemeral(behaviour_fn: impl FnOnce(libp2p_identity::Keypair) -> Self::NB) -> Self
    where
        Self: Sized;

    /// Create a new [`Swarm`] with an ephemeral identity and the `tokio` runtime.
    ///
    /// The swarm will use a [`libp2p_core::transport::MemoryTransport`] together with a
    /// [`libp2p_plaintext::Config`] authentication layer and [`libp2p_yamux::Config`] as the
    /// multiplexer. However, these details should not be relied
    /// upon by the test and may change at any time.
    #[cfg(feature = "tokio")]
    fn new_ephemeral_tokio(behaviour_fn: impl FnOnce(libp2p_identity::Keypair) -> Self::NB) -> Self
    where
        Self: Sized;

    /// Establishes a connection to the given [`Swarm`], polling both of them until the connection
    /// is established.
    ///
    /// This will take addresses from the `other` [`Swarm`] via [`Swarm::external_addresses`].
    /// By default, this iterator will not yield any addresses.
    /// To add listen addresses as external addresses, use
    /// [`ListenFuture::with_memory_addr_external`] or [`ListenFuture::with_tcp_addr_external`].
    async fn connect<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::ToSwarm: Debug;

    /// Dial the provided address and wait until a connection has been established.
    ///
    /// In a normal test scenario, you should prefer [`SwarmExt::connect`] but that is not always
    /// possible. This function only abstracts away the "dial and wait for
    /// `ConnectionEstablished` event" part.
    ///
    /// Because we don't have access to the other [`Swarm`],
    /// we can't guarantee that it makes progress.
    async fn dial_and_wait(&mut self, addr: Multiaddr) -> PeerId;

    /// Wait for specified condition to return `Some`.
    async fn wait<E, P>(&mut self, predicate: P) -> E
    where
        P: Fn(SwarmEvent<<Self::NB as NetworkBehaviour>::ToSwarm>) -> Option<E>,
        P: Send;

    /// Listens for incoming connections, polling the [`Swarm`] until the
    /// transport is ready to accept connections.
    ///
    /// The first address is for the memory transport, the second one for the TCP transport.
    fn listen(&mut self) -> ListenFuture<&mut Self>;

    /// Returns the next [`SwarmEvent`] or times out after 10 seconds.
    ///
    /// If the 10s timeout does not fit your usecase, please fall back to `StreamExt::next`.
    async fn next_swarm_event(&mut self) -> SwarmEvent<<Self::NB as NetworkBehaviour>::ToSwarm>;

    /// Returns the next behaviour event or times out after 10 seconds.
    ///
    /// If the 10s timeout does not fit your usecase, please fall back to `StreamExt::next`.
    async fn next_behaviour_event(&mut self) -> <Self::NB as NetworkBehaviour>::ToSwarm;

    async fn loop_on_next(self);
}

/// Drives two [`Swarm`]s until a certain number of events are emitted.
///
/// # Usage
///
/// ## Number of events
///
/// The number of events is configured via const generics based on the array size of the return
/// type. This allows the compiler to infer how many events you are expecting based on how you use
/// this function. For example, if you expect the first [`Swarm`] to emit 2 events, you should
/// assign the first variable of the returned tuple value to an array of size 2. This works
/// especially well if you directly pattern-match on the return value.
///
/// ## Type of event
///
/// This function utilizes the [`TryIntoOutput`] trait.
/// Similar as to the number of expected events, the type of event is inferred based on your usage.
/// If you match against a [`SwarmEvent`], the first [`SwarmEvent`] will be returned.
/// If you match against your [`NetworkBehaviour::ToSwarm`] type, [`SwarmEvent`]s which are not
/// [`SwarmEvent::Behaviour`] will be skipped until the [`Swarm`] returns a behaviour event.
///
/// You can implement the [`TryIntoOutput`] for any other type to further customize this behaviour.
///
/// # Difference to [`futures::future::join`]
///
/// This function is similar to joining two futures with two crucial differences:
/// 1. As described above, it allows you to obtain more than a single event.
/// 2. More importantly, it will continue to poll the [`Swarm`]s **even if they already has emitted
///    all expected events**.
///
/// Especially (2) is crucial for our usage of this function.
/// If a [`Swarm`] is not polled, nothing within it makes progress.
/// This can "starve" the other swarm which for example may wait for another message to be sent on a
/// connection.
///
/// Using [`drive`] instead of [`futures::future::join`] ensures that a [`Swarm`] continues to be
/// polled, even after it emitted its events.
pub async fn drive<
    TBehaviour1,
    const NUM_EVENTS_SWARM_1: usize,
    Out1,
    TBehaviour2,
    const NUM_EVENTS_SWARM_2: usize,
    Out2,
>(
    swarm1: &mut Swarm<TBehaviour2>,
    swarm2: &mut Swarm<TBehaviour1>,
) -> ([Out1; NUM_EVENTS_SWARM_1], [Out2; NUM_EVENTS_SWARM_2])
where
    TBehaviour2: NetworkBehaviour + Send,
    TBehaviour2::ToSwarm: Debug,
    TBehaviour1: NetworkBehaviour + Send,
    TBehaviour1::ToSwarm: Debug,
    SwarmEvent<TBehaviour2::ToSwarm>: TryIntoOutput<Out1>,
    SwarmEvent<TBehaviour1::ToSwarm>: TryIntoOutput<Out2>,
    Out1: Debug,
    Out2: Debug,
{
    let mut res1 = Vec::<Out1>::with_capacity(NUM_EVENTS_SWARM_1);
    let mut res2 = Vec::<Out2>::with_capacity(NUM_EVENTS_SWARM_2);

    while res1.len() < NUM_EVENTS_SWARM_1 || res2.len() < NUM_EVENTS_SWARM_2 {
        match futures::future::select(swarm1.next_swarm_event(), swarm2.next_swarm_event()).await {
            Either::Left((o1, _)) => {
                if let Ok(o1) = o1.try_into_output() {
                    res1.push(o1);
                }
            }
            Either::Right((o2, _)) => {
                if let Ok(o2) = o2.try_into_output() {
                    res2.push(o2);
                }
            }
        }
    }

    (
        res1.try_into().unwrap_or_else(|res1: Vec<_>| {
            panic!(
                "expected {NUM_EVENTS_SWARM_1} items from first swarm but got {}",
                res1.len()
            )
        }),
        res2.try_into().unwrap_or_else(|res2: Vec<_>| {
            panic!(
                "expected {NUM_EVENTS_SWARM_2} items from second swarm but got {}",
                res2.len()
            )
        }),
    )
}

pub trait TryIntoOutput<O>: Sized {
    fn try_into_output(self) -> Result<O, Self>;
}

impl<O> TryIntoOutput<O> for SwarmEvent<O> {
    fn try_into_output(self) -> Result<O, Self> {
        self.try_into_behaviour_event()
    }
}
impl<TBehaviourOutEvent> TryIntoOutput<SwarmEvent<TBehaviourOutEvent>>
    for SwarmEvent<TBehaviourOutEvent>
{
    fn try_into_output(self) -> Result<SwarmEvent<TBehaviourOutEvent>, Self> {
        Ok(self)
    }
}

#[async_trait]
impl<B> SwarmExt for Swarm<B>
where
    B: NetworkBehaviour + Send,
    <B as NetworkBehaviour>::ToSwarm: Debug,
{
    type NB = B;

    #[cfg(feature = "async-std")]
    fn new_ephemeral(behaviour_fn: impl FnOnce(libp2p_identity::Keypair) -> Self::NB) -> Self
    where
        Self: Sized,
    {
        use libp2p_core::{transport::MemoryTransport, upgrade::Version, Transport as _};
        use libp2p_identity::Keypair;

        let identity = Keypair::generate_ed25519();
        let peer_id = PeerId::from(identity.public());

        let transport = MemoryTransport::default()
            .or_transport(libp2p_tcp::async_io::Transport::default())
            .upgrade(Version::V1)
            .authenticate(libp2p_plaintext::Config::new(&identity))
            .multiplex(libp2p_yamux::Config::default())
            .timeout(Duration::from_secs(20))
            .boxed();

        Swarm::new(
            transport,
            behaviour_fn(identity),
            peer_id,
            libp2p_swarm::Config::with_async_std_executor(),
        )
    }

    #[cfg(feature = "tokio")]
    fn new_ephemeral_tokio(behaviour_fn: impl FnOnce(libp2p_identity::Keypair) -> Self::NB) -> Self
    where
        Self: Sized,
    {
        use libp2p_core::{transport::MemoryTransport, upgrade::Version, Transport as _};
        use libp2p_identity::Keypair;

        let identity = Keypair::generate_ed25519();
        let peer_id = PeerId::from(identity.public());

        let transport = MemoryTransport::default()
            .or_transport(libp2p_tcp::tokio::Transport::default())
            .upgrade(Version::V1)
            .authenticate(libp2p_plaintext::Config::new(&identity))
            .multiplex(libp2p_yamux::Config::default())
            .timeout(Duration::from_secs(20))
            .boxed();

        Swarm::new(
            transport,
            behaviour_fn(identity),
            peer_id,
            libp2p_swarm::Config::with_tokio_executor(),
        )
    }

    async fn connect<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::ToSwarm: Debug,
    {
        let external_addresses = other.external_addresses().cloned().collect();

        let dial_opts = DialOpts::peer_id(*other.local_peer_id())
            .addresses(external_addresses)
            .condition(PeerCondition::Always)
            .build();

        self.dial(dial_opts).unwrap();

        let mut dialer_done = false;
        let mut listener_done = false;

        loop {
            match futures::future::select(self.next_swarm_event(), other.next_swarm_event()).await {
                Either::Left((SwarmEvent::ConnectionEstablished { .. }, _)) => {
                    dialer_done = true;
                }
                Either::Right((SwarmEvent::ConnectionEstablished { .. }, _)) => {
                    listener_done = true;
                }
                Either::Left((other, _)) => {
                    tracing::debug!(
                        dialer=?other,
                        "Ignoring event from dialer"
                    );
                }
                Either::Right((other, _)) => {
                    tracing::debug!(
                        listener=?other,
                        "Ignoring event from listener"
                    );
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
                tracing::debug!(
                    dialer=?other,
                    "Ignoring event from dialer"
                );
                None
            }
        })
        .await
    }

    async fn wait<E, P>(&mut self, predicate: P) -> E
    where
        P: Fn(SwarmEvent<<B as NetworkBehaviour>::ToSwarm>) -> Option<E>,
        P: Send,
    {
        loop {
            let event = self.next_swarm_event().await;
            if let Some(e) = predicate(event) {
                break e;
            }
        }
    }

    fn listen(&mut self) -> ListenFuture<&mut Self> {
        ListenFuture {
            add_memory_external: false,
            add_tcp_external: false,
            swarm: self,
        }
    }

    async fn next_swarm_event(&mut self) -> SwarmEvent<<Self::NB as NetworkBehaviour>::ToSwarm> {
        match futures::future::select(
            futures_timer::Delay::new(Duration::from_secs(10)),
            self.select_next_some(),
        )
        .await
        {
            Either::Left(((), _)) => panic!("Swarm did not emit an event within 10s"),
            Either::Right((event, _)) => {
                tracing::trace!(?event);

                event
            }
        }
    }

    async fn next_behaviour_event(&mut self) -> <Self::NB as NetworkBehaviour>::ToSwarm {
        loop {
            if let Ok(event) = self.next_swarm_event().await.try_into_behaviour_event() {
                return event;
            }
        }
    }

    async fn loop_on_next(mut self) {
        while let Some(event) = self.next().await {
            tracing::trace!(?event);
        }
    }
}

pub struct ListenFuture<S> {
    add_memory_external: bool,
    add_tcp_external: bool,
    swarm: S,
}

impl<S> ListenFuture<S> {
    /// Adds the memory address we are starting to listen on as an external address using
    /// [`Swarm::add_external_address`].
    ///
    /// This is typically "safe" for tests because within a process, memory addresses are "globally"
    /// reachable. However, some tests depend on which addresses are external and need this to
    /// be configurable so it is not a good default.
    pub fn with_memory_addr_external(mut self) -> Self {
        self.add_memory_external = true;

        self
    }

    /// Adds the TCP address we are starting to listen on as an external address using
    /// [`Swarm::add_external_address`].
    ///
    /// This is typically "safe" for tests because on the same machine, 127.0.0.1 is reachable for
    /// other [`Swarm`]s. However, some tests depend on which addresses are external and need
    /// this to be configurable so it is not a good default.
    pub fn with_tcp_addr_external(mut self) -> Self {
        self.add_tcp_external = true;

        self
    }
}

impl<'s, B> IntoFuture for ListenFuture<&'s mut Swarm<B>>
where
    B: NetworkBehaviour + Send,
    <B as NetworkBehaviour>::ToSwarm: Debug,
{
    type Output = (Multiaddr, Multiaddr);
    type IntoFuture = BoxFuture<'s, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        async move {
            let swarm = self.swarm;

            let memory_addr_listener_id = swarm.listen_on(Protocol::Memory(0).into()).unwrap();

            // block until we are actually listening
            let memory_multiaddr = swarm
                .wait(|e| match e {
                    SwarmEvent::NewListenAddr {
                        address,
                        listener_id,
                    } => (listener_id == memory_addr_listener_id).then_some(address),
                    other => {
                        panic!("Unexpected event while waiting for `NewListenAddr`: {other:?}")
                    }
                })
                .await;

            let tcp_addr_listener_id = swarm
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();

            let tcp_multiaddr = swarm
                .wait(|e| match e {
                    SwarmEvent::NewListenAddr {
                        address,
                        listener_id,
                    } => (listener_id == tcp_addr_listener_id).then_some(address),
                    other => {
                        panic!("Unexpected event while waiting for `NewListenAddr`: {other:?}")
                    }
                })
                .await;

            if self.add_memory_external {
                swarm.add_external_address(memory_multiaddr.clone());
            }
            if self.add_tcp_external {
                swarm.add_external_address(tcp_multiaddr.clone());
            }

            (memory_multiaddr, tcp_multiaddr)
        }
        .boxed()
    }
}
