use std::{
    convert::identity,
    io,
    task::{Context, Poll},
    time::Duration,
};

use futures::AsyncWrite;
use futures_bounded::FuturesSet;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedOutbound},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol,
};

use crate::v2::{protocol::dial_back, DIAL_BACK_PROTOCOL};

use super::dial_request::{DialBackCommand, DialBackStatus as DialBackRes};

pub(crate) type ToBehaviour = io::Result<()>;

pub struct Handler {
    pending_nonce: Option<DialBackCommand>,
    requested_substream_nonce: Option<DialBackCommand>,
    outbound: FuturesSet<ToBehaviour>,
}

impl Handler {
    pub(crate) fn new(cmd: DialBackCommand) -> Self {
        let mut ret = Self::empty();
        ret.pending_nonce = Some(cmd);
        ret
    }

    pub(crate) fn empty() -> Self {
        Self {
            pending_nonce: None,
            requested_substream_nonce: None,
            outbound: FuturesSet::new(Duration::from_secs(10000), 2),
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = ();
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
        if let Some(cmd) = self.pending_nonce.take() {
            self.requested_substream_nonce = Some(cmd);
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(DIAL_BACK_PROTOCOL), ()),
            });
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
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => {
                if let Some(cmd) = self.requested_substream_nonce.take() {
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
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: StreamUpgradeError::NegotiationFailed | StreamUpgradeError::Timeout,
                ..
            }) => {
                if let Some(cmd) = self.requested_substream_nonce.take() {
                    let _ = cmd.back_channel.send(Err(DialBackRes::DialBackErr));
                }
            }
            _ => {}
        }
    }
}

async fn perform_dial_back(
    stream: impl AsyncWrite + Unpin,
    DialBackCommand {
        nonce,
        back_channel,
        ..
    }: DialBackCommand,
) -> io::Result<()> {
    let res = dial_back(stream, nonce)
        .await
        .map_err(|_| DialBackRes::DialBackErr)
        .map(|_| ());
    futures_time::task::sleep(futures_time::time::Duration::from_millis(100)).await;
    back_channel
        .send(res)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "send error"))?;
    Ok(())
}
