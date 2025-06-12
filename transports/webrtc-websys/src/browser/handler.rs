use std::task::Poll;

use futures::FutureExt;
use libp2p_core::{upgrade::ReadyUpgrade, PeerId};
use libp2p_swarm::{ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol};

use crate::browser::{
    protocol::signaling::ProtocolHandler, Signaling, SignalingConfig, SignalingStream,
    SIGNALING_STREAM_PROTOCOL,
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
    /// Start signaling with this peer
    InitiateSignaling,
}

/// The current status of the signaling process
/// for this handler.
#[derive(Debug, PartialEq)]
pub(crate) enum SignalingStatus {
    /// Relay connection has been established but no signaling
    /// attempts have been made
    Idle,
    /// Currently signaling (either as initiator or responder)
    Negotiating,
    /// Awaiting the initiator to start signaling
    AwaitingInitiation,
    /// Waiting before signaling retry
    WaitingForRetry,
    /// Signaling completed
    Complete,
    /// Signaling failed
    Fail,
}

#[derive(PartialEq)]
enum SignalingRole {
    Initiator,
    Responder,
}

pub struct SignalingHandler {
    /// Whether we are the initiator of the signaling protocol
    /// or responder
    role: Option<SignalingRole>,
    /// The peer we are signaling with
    peer: PeerId,
    is_dialer: bool,
    /// Current number of retries signaling has been attempted
    retry_count: u8,
    signaling_config: SignalingConfig,
    /// Current status of the signaling process
    signaling_status: SignalingStatus,
    /// Channel holding the signaling result
    signaling_result_receiver:
        Option<futures::channel::oneshot::Receiver<Result<crate::Connection, crate::Error>>>,
}

impl SignalingHandler {
    pub fn new(peer: PeerId, is_dialer: bool, config: SignalingConfig) -> Self {
        Self {
            role: None,
            peer,
            is_dialer,
            retry_count: 0,
            signaling_config: config,
            signaling_status: SignalingStatus::Idle,
            signaling_result_receiver: None,
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
                        ToBehaviourEvent::WebRTCConnectionSuccess(connection),
                    ));
                }
                Poll::Ready(Ok(Err(err))) => {
                    tracing::error!("WebRTC signaling failed: {:?}", err);
                    // If the signaling attempt failed the initiator can retry the attempt up to
                    // a configurable max retry count.
                    if self.role == Some(SignalingRole::Initiator)
                        && self.retry_count < self.signaling_config.max_signaling_retries
                    {
                        self.signaling_status = SignalingStatus::WaitingForRetry;
                        self.retry_count += 1;
                        self.role = None;

                        tracing::info!(
                            "Retrying signaling attempt {} of {} with peer {}",
                            self.retry_count,
                            self.signaling_config.max_signaling_retries,
                            self.peer
                        );

                        return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                            ToBehaviourEvent::SignalingRetry,
                        ));
                    } else {
                        self.signaling_status = SignalingStatus::Fail;
                        return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                            ToBehaviourEvent::WebRTCConnectionFailure(err),
                        ));
                    }
                }
                Poll::Ready(Err(_)) => {
                    tracing::error!("Signaling result channel dropped");
                    self.signaling_status = SignalingStatus::Fail;
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        ToBehaviourEvent::WebRTCConnectionFailure(crate::Error::Signaling(
                            "Channel dropped".to_string(),
                        )),
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
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(SIGNALING_STREAM_PROTOCOL), ()),
            });
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            FromBehaviourEvent::InitiateSignaling => {
                if self.signaling_status == SignalingStatus::Idle
                    || self.signaling_status == SignalingStatus::WaitingForRetry
                {
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
                if self.role.is_some() {
                    return;
                }

                tracing::info!("Negotiated full inbound substream for signaling");
                self.role = Some(SignalingRole::Responder);
                self.signaling_status = SignalingStatus::Negotiating;

                let _ = self.on_behaviour_event(FromBehaviourEvent::InitiateSignaling);

                let substream = fully_negotiated_inbound.protocol;
                let signaling_protocol = ProtocolHandler::new(self.signaling_config.clone());

                let (tx, rx) = futures::channel::oneshot::channel();
                self.signaling_result_receiver = Some(rx);

                wasm_bindgen_futures::spawn_local(async move {
                    let signaling_result = signaling_protocol
                        .signaling_as_responder(SignalingStream::new(substream))
                        .await;

                    let _ = tx.send(signaling_result);
                });
            }
            libp2p_swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(
                fully_negotiated_outbound,
            ) => {
                if self.role.is_some() {
                    return;
                }

                tracing::info!("Negotiated full outbound substream for signaling");
                self.role = Some(SignalingRole::Initiator);

                let substream = fully_negotiated_outbound.protocol;
                let signaling_protocol = ProtocolHandler::new(self.signaling_config.clone());

                let (tx, rx) = futures::channel::oneshot::channel();
                self.signaling_result_receiver = Some(rx);

                wasm_bindgen_futures::spawn_local(async move {
                    let signaling_result = signaling_protocol
                        .signaling_as_initiator(SignalingStream::new(substream))
                        .await;

                    let _ = tx.send(signaling_result);
                });
            }
            libp2p_swarm::handler::ConnectionEvent::DialUpgradeError(_) => {
                if self.role == Some(SignalingRole::Initiator) {
                    self.signaling_status = SignalingStatus::Fail;
                }
            }
            libp2p_swarm::handler::ConnectionEvent::ListenUpgradeError(_) => {
                if self.role == Some(SignalingRole::Responder) {
                    self.signaling_status = SignalingStatus::Fail;
                }
            }
            _ => {}
        }
    }
}
