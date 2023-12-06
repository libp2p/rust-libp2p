use std::{
    collections::VecDeque,
    convert::identity,
    io,
    task::{Context, Poll},
    time::Duration,
};

use futures::{AsyncWrite, AsyncWriteExt};
use futures_bounded::FuturesSet;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedOutbound, ProtocolsChange},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};

use crate::{request_response::DialBack, Nonce, DIAL_BACK_PROTOCOL_NAME, DIAL_BACK_UPGRADE};

use super::dial_request::{DialBack as DialBackRes, DialBackCommand};

pub(crate) type ToBehaviour = io::Result<()>;
pub(crate) type FromBehaviour = DialBackCommand;

pub struct Handler {
    pending_nonce: VecDeque<DialBackCommand>,
    requested_substream_nonce: VecDeque<DialBackCommand>,
    outbound: FuturesSet<ToBehaviour>,
}

impl Handler {
    pub(crate) fn new() -> Self {
        Self {
            pending_nonce: VecDeque::new(),
            requested_substream_nonce: VecDeque::new(),
            outbound: FuturesSet::new(Duration::from_secs(10000), 2),
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = FromBehaviour;
    type ToBehaviour = ToBehaviour;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        if let Poll::Ready(result) = self.outbound.poll_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                result
                    .map_err(|timeout| io::Error::new(io::ErrorKind::TimedOut, timeout))
                    .and_then(identity),
            ));
        }
        if let Some(cmd) = self.pending_nonce.pop_front() {
            self.requested_substream_nonce.push_back(cmd);
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(DIAL_BACK_UPGRADE, ()),
            });
        }
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        self.pending_nonce.push_back(event);
    }

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
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => {
                if let Some(cmd) = self.requested_substream_nonce.pop_front() {
                    if self
                        .outbound
                        .try_push(perform_dial_back(protocol, cmd))
                        .is_err()
                    {
                        tracing::warn!("Dial back dropped, too many requests in flight");
                    }
                } else {
                    tracing::warn!("received dial back substream without nonce");
                }
            }
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, .. }) => {
                tracing::debug!("Dial back failed: {:?}", error);
            }
            ConnectionEvent::RemoteProtocolsChange(ProtocolsChange::Added(mut protocols)) => {
                if !protocols.any(|p| p == &DIAL_BACK_PROTOCOL_NAME) {
                    todo!("handle when dialed back but not supporting protocol");
                }
            }
            e => {
                println!("e: {:?}", e);
            }
        }
    }
}

async fn perform_dial_back(
    mut stream: impl AsyncWrite + Unpin,
    DialBackCommand {
        nonce,
        back_channel,
        ..
    }: DialBackCommand,
) -> io::Result<()> {
    let res = perform_dial_back_inner(&mut stream, nonce)
        .await
        .map_err(|_| DialBackRes::DialBack)
        .map(|_| DialBackRes::Ok)
        .unwrap_or_else(|e| e);
    back_channel
        .send(res)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "send error"))?;
    Ok(())
}

async fn perform_dial_back_inner(
    mut stream: impl AsyncWrite + Unpin,
    nonce: Nonce,
) -> io::Result<()> {
    let dial_back = DialBack { nonce };
    dial_back.write_into(&mut stream).await?;
    stream.close().await?;
    Ok(())
}
