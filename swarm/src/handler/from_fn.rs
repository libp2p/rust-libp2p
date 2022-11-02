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
    state: TState,
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
        state,
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
        if self.inbound_streams.is_empty()
            && self.outbound_streams.is_empty()
            && self.pending_outbound_streams.is_empty()
        {
            return KeepAlive::No;
        }

        KeepAlive::Yes
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

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntoConnectionHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
    use libp2p_core::connection::ConnectionId;
    use libp2p_core::{Multiaddr, PeerId};

    struct MyBehaviour {
        state: Shared<State>,
    }

    #[derive(Debug, Default, Clone)]
    struct State {
        foo: String,
    }

    impl MyBehaviour {
        fn set_new_state(&mut self, foo: String) {
            self.state.foo = foo;
        }
    }

    impl NetworkBehaviour for MyBehaviour {
        type ConnectionHandler = FromFnProto<(), (), (), State>;
        type OutEvent = ();

        fn new_handler(&mut self) -> Self::ConnectionHandler {
            from_fn(
                "/foo/bar/1.0.0",
                State::default(),
                5,
                5,
                |stream, _, _, state| async move {},
                |stream, _, _, state, ()| async move {},
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
                OutEvent::InboundFinished(()) => {}
                OutEvent::OutboundFinished(()) => {}
                OutEvent::FailedToOpen(OpenError::Timeout(())) => {}
                OutEvent::FailedToOpen(OpenError::NegotiationFailed((), _neg_error)) => {}
                OutEvent::FailedToOpen(OpenError::LimitExceeded(_)) => {}
            }
        }

        fn poll(
            &mut self,
            cx: &mut Context<'_>,
            _params: &mut impl PollParameters,
        ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
            if let Poll::Ready(action) = self.state.poll(cx) {
                return Poll::Ready(action);
            }

            Poll::Pending
        }
    }

    // TODO: Add test for max pending dials
    // TODO: Add test for max inbound streams
}
