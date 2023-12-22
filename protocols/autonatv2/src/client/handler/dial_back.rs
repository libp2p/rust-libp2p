use std::{
    io,
    task::{Context, Poll},
};

use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use futures_bounded::FuturesSet;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{ConnectionEvent, FullyNegotiatedInbound, ListenUpgradeError},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use void::Void;

use crate::{request_response::DialBack, Nonce, DIAL_BACK_PROTOCOL_NAME};

use super::{DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS};

pub struct Handler {
    inbound: FuturesSet<io::Result<Nonce>>,
}

impl Handler {
    pub(crate) fn new() -> Self {
        Self {
            inbound: FuturesSet::new(DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS),
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
        if let Poll::Ready(result) = self.inbound.poll_unpin(cx) {
            match result {
                Ok(Ok(nonce)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(nonce))
                }
                Ok(Err(err)) => tracing::debug!("Dial back handler failed with: {err:?}"),
                Err(err) => tracing::debug!("Dial back handler timed out with: {err:?}"),
            }
        }
        Poll::Pending
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
                tracing::debug!("Dial back request failed: {:?}", error);
            }
            _ => {}
        }
    }
}

async fn perform_dial_back(mut stream: impl AsyncRead + AsyncWrite + Unpin) -> io::Result<u64> {
    let DialBack { nonce } = DialBack::read_from(&mut stream).await?;
    stream.close().await?;
    Ok(nonce)
}
