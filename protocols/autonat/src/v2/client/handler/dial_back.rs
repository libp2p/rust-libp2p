use std::{
    io,
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot, AsyncWriteExt};
use futures_bounded::StreamSet;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{ConnectionEvent, FullyNegotiatedInbound, ListenUpgradeError},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use void::Void;

use crate::v2::{protocol, Nonce, DIAL_BACK_PROTOCOL};

pub struct Handler {
    inbound: StreamSet<io::Result<IncomingNonce>>,
}

impl Handler {
    pub(crate) fn new() -> Self {
        Self {
            inbound: StreamSet::new(Duration::from_secs(5), 2),
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Void;
    type ToBehaviour = IncomingNonce;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(DIAL_BACK_PROTOCOL), ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        match self.inbound.poll_next_unpin(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Pending,
            Poll::Ready(Some(Err(err))) => {
                tracing::debug!("Stream timed out: {err}");
                Poll::Pending
            }
            Poll::Ready(Some(Ok(Err(err)))) => {
                tracing::debug!("Dial back handler failed with: {err:?}");
                Poll::Pending
            }
            Poll::Ready(Some(Ok(Ok(incoming_nonce)))) => {
                Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(incoming_nonce))
            }
        }
    }

    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {}

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol, ..
            }) => {
                if self.inbound.try_push(perform_dial_back(protocol)).is_err() {
                    tracing::warn!("Dial back request dropped, too many requests in flight");
                }
            }
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { error, .. }) => {
                void::unreachable(error);
            }
            _ => {}
        }
    }
}

struct State {
    stream: libp2p_swarm::Stream,
    oneshot: Option<oneshot::Receiver<io::Result<()>>>,
}

#[derive(Debug)]
pub struct IncomingNonce {
    pub nonce: Nonce,
    pub sender: oneshot::Sender<io::Result<()>>,
}

fn perform_dial_back(
    stream: libp2p_swarm::Stream,
) -> impl futures::Stream<Item = io::Result<IncomingNonce>> {
    let state = State {
        stream,
        oneshot: None,
    };
    futures::stream::unfold(state, |mut state| async move {
        if let Some(ref mut receiver) = state.oneshot {
            if receiver.await.is_err() {
                return Some((Err(io::Error::from(io::ErrorKind::Other)), state));
            }
            if let Err(e) = protocol::dial_back_response(&mut state.stream).await {
                return Some((Err(e), state));
            }
            if let Err(e) = state.stream.close().await {
                return Some((Err(e), state));
            }
            return None;
        }

        let nonce = match protocol::recv_dial_back(&mut state.stream).await {
            Ok(nonce) => nonce,
            Err(err) => {
                return Some((Err(err), state));
            }
        };

        let (sender, receiver) = oneshot::channel();
        state.oneshot = Some(receiver);
        Some((Ok(IncomingNonce { nonce, sender }), state))
    })
}
