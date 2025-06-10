use std::{task::Poll};

use futures::FutureExt;
use libp2p_core::{upgrade::ReadyUpgrade, Endpoint, PeerId};
use libp2p_swarm::{ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol};

use crate::browser::{
  Signaling, SignalingConfig, SignalingProtocol, SignalingStream, SIGNALING_STREAM_PROTOCOL
};

/// Events sent from the Handler to the Behaviour.
#[derive(Debug)]
pub enum ToBehaviourEvent {
    WebRTCConnectionSuccess(crate::Connection),
    WebRTCConnectionFailure(crate::Error),
    SignalingRetry
}

/// Events sent from the Behaviour to the Handler.
#[derive(Debug)]
pub enum FromBehaviourEvent {
    OpenSignalingStream(PeerId)
}

/// The current status of the signaling process 
/// for this handler.
#[derive(Debug, PartialEq)]
pub(crate) enum SignalingStatus {
    Idle,
    Negotiating,
    AwaitingInitiation,
    Retrying,
    Complete,
    Fail
}

pub struct SignalingHandler {
    _peer: PeerId,
    is_initiator: bool,
    retry_count: u8,
    signaling_config: SignalingConfig,
    signaling_status: SignalingStatus,
    signaling_result_receiver: Option<futures::channel::oneshot::Receiver<Result<crate::Connection, crate::Error>>>,
}

impl SignalingHandler {
    pub fn new(
        peer: PeerId,
        is_initiator: bool,
        config: SignalingConfig
    ) -> Self {
        Self {
            _peer: peer,
            is_initiator,
            retry_count: 0,
            signaling_config: config,
            signaling_status: SignalingStatus::Idle,
            signaling_result_receiver: None
        }
    }
}

impl ConnectionHandler for SignalingHandler {
    type FromBehaviour = FromBehaviourEvent;

    type ToBehaviour = ToBehaviourEvent;

    type InboundProtocol = ReadyUpgrade<StreamProtocol>;

    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;

    type InboundOpenInfo = ();

    type OutboundOpenInfo = ();

    fn listen_protocol(
        &self,
    ) -> libp2p_swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(SIGNALING_STREAM_PROTOCOL), ())
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<
        libp2p_swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        if let Some(mut receiver) = self.signaling_result_receiver.take() {
            match receiver.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(connection))) => {
                    tracing::info!("WebRTC connection established");
                    self.signaling_status = SignalingStatus::Complete;
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        ToBehaviourEvent::WebRTCConnectionSuccess(connection)
                    ));
                }
                Poll::Ready(Ok(Err(err))) => {
                    tracing::error!("WebRTC signaling failed: {:?}", err);
                    // If the signaling attempt failed the initiator can retry the attempt up to
                    // a configurable max retry count.
                    if self.is_initiator {
                        if self.retry_count < self.signaling_config.max_signaling_retries {
                            self.signaling_status = SignalingStatus::Retrying;
                            self.retry_count += 1;

                            tracing::info!("Retrying signaling attempt {} of {}", self.retry_count, self.signaling_config.max_signaling_retries);
                            
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(ToBehaviourEvent::SignalingRetry));
                        } else {
                            self.signaling_status = SignalingStatus::Fail;
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(ToBehaviourEvent::WebRTCConnectionFailure(err)));
                        }
                    }
                }
                Poll::Ready(Err(_)) => {
                    tracing::error!("Signaling result channel dropped");
                    self.signaling_status = SignalingStatus::Fail;
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        ToBehaviourEvent::WebRTCConnectionFailure(
                            crate::Error::Signaling("Channel dropped".to_string())
                        )
                    ));
                }
                Poll::Pending => {
                    // Put the signaling result channel back to poll again later
                    self.signaling_result_receiver = Some(receiver);
                }
            }
        }
        
        // Check the signaling status for this handler for any pending outbound signaling
        // attempts
        if self.signaling_status == SignalingStatus::AwaitingInitiation {
                tracing::info!("Requesting OutboundSubstream for signaling protocol");
                self.signaling_status = SignalingStatus::Negotiating;
                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(
                        ReadyUpgrade::new(SIGNALING_STREAM_PROTOCOL),
                        (),
                    ),
                });
        }


        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            FromBehaviourEvent::OpenSignalingStream(_) => {
                if self.is_initiator {
                    self.signaling_status = SignalingStatus::AwaitingInitiation;
                }

            }
        }
    }

    fn on_connection_event(
        &mut self,
        event: libp2p_swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            libp2p_swarm::handler::ConnectionEvent::FullyNegotiatedInbound(
                fully_negotiated_inbound,
) => {
                tracing::info!("Negotiated full inbound substream for signaling");
                let substream = fully_negotiated_inbound.protocol;
                let signaling_protocol = SignalingProtocol::new();
                let is_initiator = self.is_initiator; 
    
                let (tx, rx) = futures::channel::oneshot::channel();
                self.signaling_result_receiver = Some(rx);
    
                wasm_bindgen_futures::spawn_local(async move {
                    let signaling_result = signaling_protocol
                        .perform_signaling(SignalingStream::new(substream), is_initiator)
                        .await;

                    let _ = tx.send(signaling_result);
                });
            }
            libp2p_swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(
                fully_negotiated_outbound,
            ) => {
                tracing::info!("Negotiated full outbound substream for signaling");
                let substream = fully_negotiated_outbound.protocol;
                let signaling_protocol = SignalingProtocol::new();
                let is_initiator = self.is_initiator;

                let (tx, rx) = futures::channel::oneshot::channel();
                self.signaling_result_receiver = Some(rx);

                wasm_bindgen_futures::spawn_local(async move {
                    let signaling_result = signaling_protocol
                        .perform_signaling(SignalingStream::new(substream), is_initiator)
                        .await;
                   
                    let _ = tx.send(signaling_result);
    
                });
            }
            _ => {}
        }
    }
}
