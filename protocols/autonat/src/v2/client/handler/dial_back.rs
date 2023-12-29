use std::{
    io,
    task::{Context, Poll},
    time::Duration,
};

use futures_bounded::FuturesSet;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{ConnectionEvent, FullyNegotiatedInbound, ListenUpgradeError},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use void::Void;

use crate::v2::{protocol, Nonce, DIAL_BACK_PROTOCOL_NAME};

pub struct Handler {
    inbound: FuturesSet<io::Result<Nonce>>,
}

impl Handler {
    pub(crate) fn new() -> Self {
        Self {
            inbound: FuturesSet::new(Duration::from_secs(5), 2),
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Void;
    type ToBehaviour = Nonce;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(DIAL_BACK_PROTOCOL_NAME), ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        match self.inbound.poll_unpin(cx) {
            Poll::Ready(Ok(Ok(nonce))) => {
                Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(nonce))
            }
            Poll::Ready(Ok(Err(err))) => {
                tracing::debug!("Dial back handler failed with: {err:?}");
                Poll::Pending
            }
            Poll::Ready(Err(err)) => {
                tracing::debug!("Dial back handler timed out with: {err:?}");
                Poll::Pending
            }
            Poll::Pending => Poll::Pending,
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
                if self
                    .inbound
                    .try_push(protocol::recv_dial_back(protocol))
                    .is_err()
                {
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
