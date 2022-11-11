use crate::handler::{InboundUpgradeSend, OutboundUpgradeSend};
use crate::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, IntoConnectionHandler,
    KeepAlive, NegotiatedSubstream, NetworkBehaviourAction, NotifyHandler, SubstreamProtocol,
};
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p_core::connection::ConnectionId;
use libp2p_core::upgrade::{NegotiationError, ReadyUpgrade};
use libp2p_core::{ConnectedPoint, PeerId, UpgradeError};
use std::collections::{HashSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};
use void::Void;

/// A low-level building block for protocols that can be expressed as async functions.
///
/// An async function in Rust is executed within an executor and thus only has access to its local state
/// and by extension, the state that was available when the [`Future`] was constructed.
///
/// This [`ConnectionHandler`] aims to reduce the boilerplate for protocols which can be expressed as
/// a static sequence of reads and writes to a socket where the response can be generated with limited
/// "local" knowledge or in other words, the local state within the [`Future`].
///
/// For outbound substreams, arbitrary data can be supplied via [`InEvent::NewOutbound`] which will be
/// made available to the callback once the stream is fully negotiated.
///
/// Inbound substreams may be opened at any time by the remote. To facilitate this one and more usecases,
/// the supplied callbacks for inbound and outbound substream are given access to the handler's `state`
/// field. This `state` field can contain arbitrary data and can be updated by the [`NetworkBehaviour`]
/// via [`InEvent::UpdateState`].
///
/// The design of this [`ConnectionHandler`] trades boilerplate (you don't have to write your own handler)
/// and simplicity (small API surface) for eventual consistency, depending on your protocol design:
///
/// Most likely, the [`NetworkBehaviour`] is the authoritive source of `TState` but updates to it have
/// to be manually performed via [`InEvent::UpdateState`]. Thus, the state given to newly created
/// substreams, may be outdated and only eventually-consistent.
///
/// [`NetworkBehaviour`]: crate::NetworkBehaviour
pub fn from_fn<TInbound, TOutbound, TOutboundOpenInfo, TState, TInboundFuture, TOutboundFuture>(
    protocol: &'static str,
    state: &Shared<TState>,
    inbound_streams_limit: usize,
    pending_dial_limit: usize,
    on_new_inbound: impl Fn(NegotiatedSubstream, PeerId, &ConnectedPoint, &TState) -> TInboundFuture
        + Send
        + 'static,
    on_new_outbound: impl Fn(
            NegotiatedSubstream,
            PeerId,
            &ConnectedPoint,
            &TState,
            TOutboundOpenInfo,
        ) -> TOutboundFuture
        + Send
        + 'static,
) -> FromFnProto<TInbound, TOutbound, TOutboundOpenInfo, TState>
where
    TState: Clone,
    TInboundFuture: Future<Output = TInbound> + Send + 'static,
    TOutboundFuture: Future<Output = TOutbound> + Send + 'static,
{
    FromFnProto {
        protocol,
        inbound_streams_limit,
        pending_outbound_streams_limit: pending_dial_limit,
        on_new_inbound: Box::new(move |stream, remote_peer_id, connected_point, state| {
            on_new_inbound(stream, remote_peer_id, connected_point, state).boxed()
        }),
        on_new_outbound: Box::new(
            move |stream, remote_peer_id, connected_point, state, info| {
                on_new_outbound(stream, remote_peer_id, connected_point, state, info).boxed()
            },
        ),
        state: state.inner.clone(),
    }
}

#[derive(Debug)]
pub enum OutEvent<I, O, OpenInfo> {
    InboundFinished(I),
    OutboundFinished(O),
    FailedToOpen(OpenError<OpenInfo>),
}

#[derive(Debug)]
pub enum OpenError<OpenInfo> {
    Timeout(OpenInfo),
    LimitExceeded(OpenInfo),
    NegotiationFailed(OpenInfo, NegotiationError),
}

/// A wrapper for state that is shared across all connections.
///
/// Any update to the state will "automatically" be relayed to all connections, assuming this struct
/// is correctly wired into your [`NetworkBehaviour`](crate::swarm::NetworkBehaviour).
///
/// This struct implements an observer pattern. All registered connections will receive updates that
/// are made to the state.
pub struct Shared<T> {
    inner: T,

    dirty: bool,
    waker: Option<Waker>,
    connections: HashSet<(PeerId, ConnectionId)>,
    pending_update_events: VecDeque<(PeerId, ConnectionId, T)>,
}

impl<T> Shared<T>
where
    T: Clone,
{
    pub fn new(state: T) -> Self {
        Self {
            inner: state,
            dirty: false,
            waker: None,
            connections: HashSet::default(),
            pending_update_events: VecDeque::default(),
        }
    }

    pub fn register_connection(&mut self, peer_id: PeerId, id: ConnectionId) {
        self.connections.insert((peer_id, id));
    }

    pub fn unregister_connection(&mut self, peer_id: PeerId, id: ConnectionId) {
        self.connections.remove(&(peer_id, id));
    }

    pub fn poll<TOut, THandler, TOpenInfo>(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<NetworkBehaviourAction<TOut, THandler, InEvent<T, TOpenInfo>>>
    where
        THandler: IntoConnectionHandler,
    {
        if self.dirty {
            self.pending_update_events = self
                .connections
                .iter()
                .map(|(peer_id, conn_id)| (*peer_id, *conn_id, self.inner.clone()))
                .collect();

            self.dirty = false;
        }

        if let Some((peer_id, conn_id, state)) = self.pending_update_events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                peer_id,
                handler: NotifyHandler::One(conn_id),
                event: InEvent::UpdateState(state),
            });
        }

        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        &mut self.inner
    }
}

impl<OpenInfo> fmt::Display for OpenError<OpenInfo> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Timeout(_) => write!(f, "opening new substream timed out"),
            OpenError::LimitExceeded(_) => write!(f, "limit for pending dials exceeded"),
            OpenError::NegotiationFailed(_, _) => Ok(()), // Don't print anything to avoid double printing of error.
        }
    }
}

impl<OpenInfo> Error for OpenError<OpenInfo>
where
    OpenInfo: fmt::Debug,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            OpenError::Timeout(_) => None,
            OpenError::LimitExceeded(_) => None,
            OpenError::NegotiationFailed(_, source) => Some(source),
        }
    }
}

#[derive(Debug)]
pub enum InEvent<TState, TOutboundOpenInfo> {
    UpdateState(TState),
    NewOutbound(TOutboundOpenInfo),
}

pub struct FromFnProto<TInbound, TOutbound, TOutboundOpenInfo, TState> {
    protocol: &'static str,

    on_new_inbound: Box<
        dyn Fn(
                NegotiatedSubstream,
                PeerId,
                &ConnectedPoint,
                &TState,
            ) -> BoxFuture<'static, TInbound>
            + Send,
    >,
    on_new_outbound: Box<
        dyn Fn(
                NegotiatedSubstream,
                PeerId,
                &ConnectedPoint,
                &TState,
                TOutboundOpenInfo,
            ) -> BoxFuture<'static, TOutbound>
            + Send,
    >,

    inbound_streams_limit: usize,
    pending_outbound_streams_limit: usize,

    state: TState,
}

impl<TInbound, TOutbound, TOutboundOpenInfo, TState> IntoConnectionHandler
    for FromFnProto<TInbound, TOutbound, TOutboundOpenInfo, TState>
where
    TInbound: fmt::Debug + Send + 'static,
    TOutbound: fmt::Debug + Send + 'static,
    TOutboundOpenInfo: fmt::Debug + Send + 'static,
    TState: fmt::Debug + Send + 'static,
{
    type Handler = FromFn<TInbound, TOutbound, TOutboundOpenInfo, TState>;

    fn into_handler(
        self,
        remote_peer_id: &PeerId,
        connected_point: &ConnectedPoint,
    ) -> Self::Handler {
        FromFn {
            protocol: self.protocol,
            remote_peer_id: *remote_peer_id,
            connected_point: connected_point.clone(),
            inbound_streams: FuturesUnordered::default(),
            outbound_streams: FuturesUnordered::default(),
            on_new_inbound: self.on_new_inbound,
            on_new_outbound: self.on_new_outbound,
            inbound_streams_limit: self.inbound_streams_limit,
            pending_outbound_streams: VecDeque::default(),
            pending_outbound_streams_limit: self.pending_outbound_streams_limit,
            failed_open: VecDeque::default(),
            state: self.state,
            keep_alive: KeepAlive::Yes,
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
        ReadyUpgrade::new(self.protocol)
    }
}

pub struct FromFn<TInbound, TOutbound, TOutboundInfo, TState> {
    protocol: &'static str,
    remote_peer_id: PeerId,
    connected_point: ConnectedPoint,

    inbound_streams: FuturesUnordered<BoxFuture<'static, TInbound>>,
    outbound_streams: FuturesUnordered<BoxFuture<'static, TOutbound>>,

    on_new_inbound: Box<
        dyn Fn(
                NegotiatedSubstream,
                PeerId,
                &ConnectedPoint,
                &TState,
            ) -> BoxFuture<'static, TInbound>
            + Send,
    >,
    on_new_outbound: Box<
        dyn Fn(
                NegotiatedSubstream,
                PeerId,
                &ConnectedPoint,
                &TState,
                TOutboundInfo,
            ) -> BoxFuture<'static, TOutbound>
            + Send,
    >,

    inbound_streams_limit: usize,

    pending_outbound_streams: VecDeque<TOutboundInfo>,
    pending_outbound_streams_limit: usize,

    failed_open: VecDeque<OpenError<TOutboundInfo>>,

    state: TState,

    keep_alive: KeepAlive,
}

impl<TInbound, TOutbound, TOutboundInfo, TState> ConnectionHandler
    for FromFn<TInbound, TOutbound, TOutboundInfo, TState>
where
    TOutboundInfo: fmt::Debug + Send + 'static,
    TInbound: fmt::Debug + Send + 'static,
    TOutbound: fmt::Debug + Send + 'static,
    TState: fmt::Debug + Send + 'static,
{
    type InEvent = InEvent<TState, TOutboundInfo>;
    type OutEvent = OutEvent<TInbound, TOutbound, TOutboundInfo>;
    type Error = Void;
    type InboundProtocol = ReadyUpgrade<&'static str>;
    type OutboundProtocol = ReadyUpgrade<&'static str>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = TOutboundInfo;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(self.protocol), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        _: Self::InboundOpenInfo,
    ) {
        if self.inbound_streams.len() >= self.inbound_streams_limit {
            log::debug!(
                "Dropping inbound substream because limit ({}) would be exceeded",
                self.inbound_streams_limit
            );
            return;
        }

        let inbound_future = (self.on_new_inbound)(
            protocol,
            self.remote_peer_id,
            &self.connected_point,
            &mut self.state,
        );
        self.inbound_streams.push(inbound_future);
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        let outbound_future = (self.on_new_outbound)(
            protocol,
            self.remote_peer_id,
            &self.connected_point,
            &mut self.state,
            info,
        );
        self.outbound_streams.push(outbound_future);
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            InEvent::UpdateState(new_state) => self.state = new_state,
            InEvent::NewOutbound(open_info) => {
                if self.pending_outbound_streams.len() >= self.pending_outbound_streams_limit {
                    self.failed_open
                        .push_back(OpenError::LimitExceeded(open_info));
                } else {
                    self.pending_outbound_streams.push_back(open_info);
                }
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ConnectionHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        match error {
            ConnectionHandlerUpgrErr::Timeout => {
                self.failed_open.push_back(OpenError::Timeout(info))
            }
            ConnectionHandlerUpgrErr::Timer => {}
            ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(negotiation)) => self
                .failed_open
                .push_back(OpenError::NegotiationFailed(info, negotiation)),
            ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(apply)) => {
                void::unreachable(apply)
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(error) = self.failed_open.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::FailedToOpen(
                error,
            )));
        }

        match self.outbound_streams.poll_next_unpin(cx) {
            Poll::Ready(Some(outbound_done)) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::OutboundFinished(
                    outbound_done,
                )));
            }
            Poll::Ready(None) => {
                // Normally, we'd register a waker here but `Connection` polls us anyway again
                // after calling `inject` on us which is where we'd use the waker.
            }
            Poll::Pending => {}
        };

        match self.inbound_streams.poll_next_unpin(cx) {
            Poll::Ready(Some(inbound_done)) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::InboundFinished(
                    inbound_done,
                )));
            }
            Poll::Ready(None) => {
                // Normally, we'd register a waker here but `Connection` polls us anyway again
                // after calling `inject` on us which is where we'd use the waker.
            }
            Poll::Pending => {}
        };

        if let Some(outbound_open_info) = self.pending_outbound_streams.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    ReadyUpgrade::new(self.protocol),
                    outbound_open_info,
                ),
            });
        }

        if self.inbound_streams.is_empty()
            && self.outbound_streams.is_empty()
            && self.pending_outbound_streams.is_empty()
        {
            if self.keep_alive.is_yes() {
                // TODO: Make timeout configurable
                self.keep_alive = KeepAlive::Until(Instant::now() + Duration::from_secs(10))
            }
        } else {
            self.keep_alive = KeepAlive::Yes
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IntoConnectionHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters, Swarm,
        SwarmEvent,
    };
    use futures::{AsyncReadExt, AsyncWriteExt};
    use libp2p::plaintext::PlainText2Config;
    use libp2p::yamux;
    use libp2p_core::connection::ConnectionId;
    use libp2p_core::transport::MemoryTransport;
    use libp2p_core::upgrade::Version;
    use libp2p_core::{identity, Multiaddr, PeerId, Transport};
    use std::collections::HashMap;
    use std::io;
    use std::ops::AddAssign;

    #[async_std::test]
    async fn greetings() {
        let _ = env_logger::try_init();

        let mut alice = make_swarm("Alice");
        let mut bob = make_swarm("Bob");

        let bob_peer_id = *bob.local_peer_id();
        let listen_id = alice.listen_on("/memory/0".parse().unwrap()).unwrap();

        let alice_listen_addr = loop {
            if let SwarmEvent::NewListenAddr {
                address,
                listener_id,
            } = alice.select_next_some().await
            {
                if listener_id == listen_id {
                    break address;
                }
            }
        };
        bob.dial(alice_listen_addr).unwrap();

        futures::future::join(
            async {
                while !matches!(
                    alice.select_next_some().await,
                    SwarmEvent::ConnectionEstablished { .. }
                ) {}
            },
            async {
                while !matches!(
                    bob.select_next_some().await,
                    SwarmEvent::ConnectionEstablished { .. }
                ) {}
            },
        )
        .await;

        futures::future::join(
            async {
                alice.behaviour_mut().say_hello(bob_peer_id);

                loop {
                    if let SwarmEvent::Behaviour(greetings) = alice.select_next_some().await {
                        assert_eq!(*greetings.get(&Name("Bob".to_owned())).unwrap(), 1);
                        break;
                    }
                }
            },
            async {
                loop {
                    if let SwarmEvent::Behaviour(greetings) = bob.select_next_some().await {
                        assert_eq!(*greetings.get(&Name("Alice".to_owned())).unwrap(), 1);
                        break;
                    }
                }
            },
        )
        .await;

        alice.behaviour_mut().state.name = Name("Carol".to_owned());
        bob.behaviour_mut().state.name = Name("Steve".to_owned());

        futures::future::join(
            async {
                alice.behaviour_mut().say_hello(bob_peer_id);

                loop {
                    if let SwarmEvent::Behaviour(greetings) = alice.select_next_some().await {
                        assert_eq!(*greetings.get(&Name("Bob".to_owned())).unwrap(), 1);
                        assert_eq!(*greetings.get(&Name("Steve".to_owned())).unwrap(), 1);
                        break;
                    }
                }
            },
            async {
                loop {
                    if let SwarmEvent::Behaviour(greetings) = bob.select_next_some().await {
                        assert_eq!(*greetings.get(&Name("Alice".to_owned())).unwrap(), 1);
                        assert_eq!(*greetings.get(&Name("Carol".to_owned())).unwrap(), 1);
                        break;
                    }
                }
            },
        )
        .await;
    }

    struct HelloBehaviour {
        state: Shared<State>,
        pending_messages: VecDeque<PeerId>,
        pending_events: VecDeque<HashMap<Name, u8>>,
        greeting_count: HashMap<Name, u8>,
    }

    #[derive(Debug, Clone)]
    struct State {
        name: Name,
    }

    #[derive(Debug, Clone, PartialEq, Hash, Eq)]
    struct Name(String);

    impl HelloBehaviour {
        fn say_hello(&mut self, to: PeerId) {
            self.pending_messages.push_back(to);
        }
    }

    impl NetworkBehaviour for HelloBehaviour {
        type ConnectionHandler = FromFnProto<io::Result<Name>, io::Result<Name>, (), State>;
        type OutEvent = HashMap<Name, u8>;

        fn new_handler(&mut self) -> Self::ConnectionHandler {
            from_fn(
                "/hello/1.0.0",
                &self.state,
                5,
                5,
                |mut stream, _, _, state| {
                    let my_name = state.name.to_owned();

                    async move {
                        let mut received_name = Vec::new();
                        stream.read_to_end(&mut received_name).await?;

                        stream.write_all(&my_name.0.as_bytes()).await?;
                        stream.close().await?;

                        Ok(Name(String::from_utf8(received_name).unwrap()))
                    }
                },
                |mut stream, _, _, state, _| {
                    let my_name = state.name.to_owned();

                    async move {
                        stream.write_all(&my_name.0.as_bytes()).await?;
                        stream.flush().await?;
                        stream.close().await?;

                        let mut received_name = Vec::new();
                        stream.read_to_end(&mut received_name).await?;

                        Ok(Name(String::from_utf8(received_name).unwrap()))
                    }
                },
            )
        }

        fn inject_connection_established(
            &mut self,
            peer_id: &PeerId,
            connection_id: &ConnectionId,
            _endpoint: &ConnectedPoint,
            _failed_addresses: Option<&Vec<Multiaddr>>,
            _other_established: usize,
        ) {
            self.state.register_connection(*peer_id, *connection_id);
        }

        fn inject_connection_closed(
            &mut self,
            peer_id: &PeerId,
            connection_id: &ConnectionId,
            _: &ConnectedPoint,
            _: <Self::ConnectionHandler as IntoConnectionHandler>::Handler,
            _remaining_established: usize,
        ) {
            self.state.unregister_connection(*peer_id, *connection_id);
        }

        fn inject_event(
            &mut self,
            _peer_id: PeerId,
            _connection: ConnectionId,
            event: <<Self::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
        ) {
            match event {
                OutEvent::InboundFinished(Ok(name)) => {
                    self.greeting_count.entry(name).or_default().add_assign(1);

                    self.pending_events.push_back(self.greeting_count.clone())
                }
                OutEvent::OutboundFinished(Ok(name)) => {
                    self.greeting_count.entry(name).or_default().add_assign(1);

                    self.pending_events.push_back(self.greeting_count.clone())
                }
                OutEvent::InboundFinished(_) => {}
                OutEvent::OutboundFinished(_) => {}
                OutEvent::FailedToOpen(OpenError::Timeout(_)) => {}
                OutEvent::FailedToOpen(OpenError::NegotiationFailed(_, _neg_error)) => {}
                OutEvent::FailedToOpen(OpenError::LimitExceeded(_)) => {}
            }
        }

        fn poll(
            &mut self,
            cx: &mut Context<'_>,
            _: &mut impl PollParameters,
        ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
            if let Some(greeting_count) = self.pending_events.pop_front() {
                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(greeting_count));
            }

            if let Poll::Ready(action) = self.state.poll(cx) {
                return Poll::Ready(action);
            }

            if let Some(to) = self.pending_messages.pop_front() {
                return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                    peer_id: to,
                    handler: NotifyHandler::Any,
                    event: InEvent::NewOutbound(()),
                });
            }

            Poll::Pending
        }
    }

    fn make_swarm(name: &'static str) -> Swarm<HelloBehaviour> {
        let identity = identity::Keypair::generate_ed25519();

        let transport = MemoryTransport::new()
            .upgrade(Version::V1)
            .authenticate(PlainText2Config {
                local_public_key: identity.public(),
            })
            .multiplex(yamux::YamuxConfig::default())
            .boxed();

        let swarm = Swarm::new(
            transport,
            HelloBehaviour {
                state: Shared::new(State {
                    name: Name(name.to_owned()),
                }),
                pending_messages: Default::default(),
                pending_events: Default::default(),
                greeting_count: Default::default(),
            },
            identity.public().to_peer_id(),
        );
        swarm
    }

    // TODO: Add test for max pending dials
    // TODO: Add test for max inbound streams
}
