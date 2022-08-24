use crate::handler::{InboundUpgradeSend, OutboundUpgradeSend};
use crate::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    NegotiatedSubstream, SubstreamProtocol,
};
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p_core::upgrade::{NegotiationError, ReadyUpgrade};
use libp2p_core::UpgradeError;
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::task::{Context, Poll, Waker};
use void::Void;

pub fn from_fn<TInbound, TOutbound, TOutboundOpenInfo, TInboundFuture, TOutboundFuture>(
    protocol: &'static str,
    on_new_inbound: impl Fn(NegotiatedSubstream) -> TInboundFuture + Send + 'static,
    on_new_outbound: impl Fn(NegotiatedSubstream, TOutboundOpenInfo) -> TOutboundFuture + Send + 'static,
) -> FromFn<TInbound, TOutbound, TOutboundOpenInfo>
where
    TInboundFuture: Future<Output = TInbound> + Send + 'static,
    TOutboundFuture: Future<Output = TOutbound> + Send + 'static,
{
    FromFn {
        protocol,
        inbound_streams: FuturesUnordered::default(),
        outbound_streams: FuturesUnordered::default(),
        on_new_inbound: Box::new(move |stream| on_new_inbound(stream).boxed()),
        on_new_outbound: Box::new(move |stream, info| on_new_outbound(stream, info).boxed()),
        idle_waker: None,
        pending_dials: VecDeque::default(),
        failed_open: VecDeque::default(),
    }
}

#[derive(Debug)]
pub enum OutEvent<I, O, OpenInfo> {
    InboundFinished(I),
    OutboundFinished(O),
    FailedToOpen(OpenError<OpenInfo>),
}

// TODO: Impl std::error::Error
#[derive(Debug)]
pub enum OpenError<OpenInfo> {
    Timeout(OpenInfo),
    NegotiationFailed(OpenInfo, NegotiationError),
}

#[derive(Default, Debug)]
pub struct NewOutbound<I>(pub I);

pub struct FromFn<TInbound, TOutbound, TOutboundInfo> {
    protocol: &'static str,

    inbound_streams: FuturesUnordered<BoxFuture<'static, TInbound>>,
    outbound_streams: FuturesUnordered<BoxFuture<'static, TOutbound>>,

    on_new_inbound: Box<dyn Fn(NegotiatedSubstream) -> BoxFuture<'static, TInbound> + Send>,
    on_new_outbound:
        Box<dyn Fn(NegotiatedSubstream, TOutboundInfo) -> BoxFuture<'static, TOutbound> + Send>,

    idle_waker: Option<Waker>,

    pending_dials: VecDeque<TOutboundInfo>,

    failed_open: VecDeque<OpenError<TOutboundInfo>>,
}

impl<TInbound, TOutbound, TOutboundInfo> ConnectionHandler
    for FromFn<TInbound, TOutbound, TOutboundInfo>
where
    TOutboundInfo: fmt::Debug + Send + 'static,
    TInbound: fmt::Debug + Send + 'static,
    TOutbound: fmt::Debug + Send + 'static,
{
    type InEvent = NewOutbound<TOutboundInfo>;
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
        self.inbound_streams.push((self.on_new_inbound)(protocol));

        if let Some(waker) = self.idle_waker.take() {
            waker.wake();
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        self.outbound_streams
            .push((self.on_new_outbound)(protocol, info));

        if let Some(waker) = self.idle_waker.take() {
            waker.wake();
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        self.pending_dials.push_back(event.0);
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
            && self.pending_dials.is_empty()
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

        if let Some(outbound_open_info) = self.pending_dials.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    ReadyUpgrade::new(self.protocol),
                    outbound_open_info,
                ),
            });
        }

        match self.outbound_streams.poll_next_unpin(cx) {
            Poll::Ready(Some(outbound_done)) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::OutboundFinished(
                    outbound_done,
                )));
            }
            Poll::Ready(None) => {
                self.idle_waker = Some(cx.waker().clone());
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
                self.idle_waker = Some(cx.waker().clone());
            }
            Poll::Pending => {}
        };

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IntoConnectionHandler, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler,
        PollParameters,
    };
    use libp2p_core::connection::ConnectionId;
    use libp2p_core::PeerId;

    struct MyBehaviour {}

    impl NetworkBehaviour for MyBehaviour {
        type ConnectionHandler = FromFn<(), (), ()>;
        type OutEvent = ();

        fn new_handler(&mut self) -> Self::ConnectionHandler {
            from_fn(
                "/foo/bar/1.0.0",
                |_stream| async move {},
                |_stream, ()| async move {},
            )
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
                OutEvent::FailedToOpen(OpenError::NegotiationFailed((), neg_error)) => {}
            }
        }

        fn poll(
            &mut self,
            _cx: &mut Context<'_>,
            _params: &mut impl PollParameters,
        ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
            Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                peer_id: PeerId::random(),
                handler: NotifyHandler::Any,
                event: NewOutbound::default(),
            })
        }
    }
}
