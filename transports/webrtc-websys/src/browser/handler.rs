use std::task::Poll;

use futures::FutureExt;
use libp2p_core::{upgrade::ReadyUpgrade, PeerId};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use tracing::{info, instrument};

use crate::browser::{
    protocol::signaling::ProtocolHandler, Signaling, SignalingConfig, SignalingStream,
    SIGNALING_STREAM_PROTOCOL,
};

/// Events sent from the Handler to the Behaviour.
#[derive(Debug)]
pub enum ToBehaviourEvent {
    WebRTCConnectionSuccess(crate::Connection),
    WebRTCConnectionFailure(crate::Error),
    SignalingRetry,
}

/// Events sent from the Behaviour to the Handler.
#[derive(Debug)]
pub enum FromBehaviourEvent {
    /// Start signaling with this peer
    InitiateSignaling,
}

/// The current status of the signaling process for this handler.
#[derive(Debug, PartialEq)]
pub(crate) enum SignalingStatus {
    /// Relay connection has been established but no signaling attempts have been made
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

enum HandlerType {
    /// Handler for signaling over relay
    Signaling {
        role: Option<SignalingRole>,
        retry_count: u8,
        signaling_config: SignalingConfig,
        signaling_status: SignalingStatus,
        signaling_result_receiver: Option<futures::channel::oneshot::Receiver<Result<crate::Connection, crate::Error>>>,
    },
    /// Handler for established WebRTC connection
    EstablishedWebRTC,
    /// No-op handler
    Noop,
}

pub struct SignalingHandler {
    local_peer_id: PeerId,
    peer: PeerId,
    is_dialer: bool,
    handler_type: HandlerType,
}

impl SignalingHandler {
    pub fn new(
        local_peer_id: PeerId,
        peer: PeerId,
        is_dialer: bool,
        config: SignalingConfig,
        is_noop: bool,
    ) -> Self {
        let handler_type = if is_noop {
            HandlerType::Noop
        } else {
            HandlerType::Signaling {
                role: None,
                retry_count: 0,
                signaling_config: config,
                signaling_status: SignalingStatus::Idle,
                signaling_result_receiver: None,
            }
        };

        Self {
            local_peer_id,
            peer,
            is_dialer,
            handler_type,
        }
    }

    /// Create a handler for an already established WebRTC connection
    pub fn new_established_webrtc(local_peer_id: PeerId, peer: PeerId) -> Self {
        Self {
            local_peer_id,
            peer,
            is_dialer: false,
            handler_type: HandlerType::EstablishedWebRTC,
        }
    }

    fn should_be_initiator(&self, remote_peer: &PeerId) -> bool {
        // Use lexicographic comparison of peer IDs for deterministic role assignment
        self.local_peer_id < *remote_peer
    }

    /// Check if signaling should be retried based on the error and current state
    fn should_retry(&self, _error: &crate::Error) -> bool {
        match &self.handler_type {
            HandlerType::Signaling { role, retry_count, signaling_config, .. } => {
                *role == Some(SignalingRole::Initiator)
                    && *retry_count < signaling_config.max_signaling_retries
            }
            _ => false,
        }
    }

    /// Handle successful signaling completion
    fn handle_signaling_success(
        &mut self,
        connection: crate::Connection,
    ) -> ConnectionHandlerEvent<
        <Self as ConnectionHandler>::OutboundProtocol,
        <Self as ConnectionHandler>::OutboundOpenInfo,
        <Self as ConnectionHandler>::ToBehaviour,
    > {
        tracing::info!("WebRTC connection established with peer {}", self.peer);

        if let HandlerType::Signaling { signaling_status, signaling_result_receiver, .. } = &mut self.handler_type {
            *signaling_status = SignalingStatus::Complete;
            *signaling_result_receiver = None;
        }

        ConnectionHandlerEvent::NotifyBehaviour(ToBehaviourEvent::WebRTCConnectionSuccess(
            connection,
        ))
    }

    /// Handle signaling failure with potential retry logic
    fn handle_signaling_failure(
        &mut self,
        error: crate::Error,
    ) -> ConnectionHandlerEvent<
        <Self as ConnectionHandler>::OutboundProtocol,
        <Self as ConnectionHandler>::OutboundOpenInfo,
        <Self as ConnectionHandler>::ToBehaviour,
    > {
        if self.should_retry(&error) {
            if let HandlerType::Signaling { retry_count, signaling_config, .. } = &self.handler_type {
                tracing::info!(
                    "Retrying signaling attempt {} of {} with peer {}",
                    retry_count + 1,
                    signaling_config.max_signaling_retries,
                    self.peer
                );
            }

            self.reset_for_retry();
            ConnectionHandlerEvent::NotifyBehaviour(ToBehaviourEvent::SignalingRetry)
        } else {
            tracing::error!("WebRTC signaling failed permanently: {:?}", error);

            if let HandlerType::Signaling { signaling_status, .. } = &mut self.handler_type {
                *signaling_status = SignalingStatus::Fail;
            }

            ConnectionHandlerEvent::NotifyBehaviour(ToBehaviourEvent::WebRTCConnectionFailure(
                error,
            ))
        }
    }

    /// Reset handler state for retry
    fn reset_for_retry(&mut self) {
        if let HandlerType::Signaling {
            signaling_status,
            retry_count,
            role,
            signaling_result_receiver,
            ..
        } = &mut self.handler_type {
            *signaling_status = SignalingStatus::WaitingForRetry;
            *retry_count += 1;
            *role = None;
            *signaling_result_receiver = None;
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
        match &mut self.handler_type {
            HandlerType::Noop | HandlerType::EstablishedWebRTC => Poll::Pending,
            HandlerType::Signaling {
                signaling_status,
                signaling_result_receiver,
                ..
            } => {
                if *signaling_status == SignalingStatus::Complete {
                    return Poll::Pending;
                }

                if let Some(mut receiver) = signaling_result_receiver.take() {
                    match receiver.poll_unpin(cx) {
                        Poll::Ready(Ok(Ok(connection))) => {
                            return Poll::Ready(self.handle_signaling_success(connection));
                        }
                        Poll::Ready(Ok(Err(err))) => {
                            tracing::error!("WebRTC signaling failed: {:?}", err);
                            return Poll::Ready(self.handle_signaling_failure(err));
                        }
                        Poll::Ready(Err(_)) => {
                            tracing::error!("Signaling result channel dropped");
                            let error = crate::Error::Signaling(
                                "Signaling channel dropped unexpectedly".to_string(),
                            );
                            return Poll::Ready(self.handle_signaling_failure(error));
                        }
                        Poll::Pending => {
                            *signaling_result_receiver = Some(receiver);
                        }
                    }
                }

                if *signaling_status == SignalingStatus::AwaitingInitiation {
                    *signaling_status = SignalingStatus::Negotiating;
                    return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            ReadyUpgrade::new(SIGNALING_STREAM_PROTOCOL),
                            (),
                        ),
                    });
                }

                Poll::Pending
            }
        }
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        let should_be_initiator = self.should_be_initiator(&self.peer);
        match (&mut self.handler_type, event) {
            (HandlerType::Signaling { signaling_status, .. }, FromBehaviourEvent::InitiateSignaling) => {
                if *signaling_status == SignalingStatus::Idle
                    || *signaling_status == SignalingStatus::WaitingForRetry
                {
                    if should_be_initiator {
                        tracing::info!("Initiating signaling with peer {} (I am initiator)", self.peer);
                    *signaling_status = SignalingStatus::AwaitingInitiation;
                    } else {
                         tracing::info!("Waiting for peer {} to initiate signaling (I am responder)", self.peer);
                    *signaling_status = SignalingStatus::Negotiating;
                    }
                }
            }
            _ => {}
        }
    }

    #[instrument(skip(self))]
    fn on_connection_event(
    &mut self,
    event: libp2p_swarm::handler::ConnectionEvent<
        Self::InboundProtocol,
        Self::OutboundProtocol,
        Self::InboundOpenInfo,
        Self::OutboundOpenInfo,
    >,
) {
    let should_be_initiator = self.should_be_initiator(&self.peer);
    match &mut self.handler_type {
        HandlerType::Noop | HandlerType::EstablishedWebRTC => return,
        HandlerType::Signaling {
            role,
            signaling_status,
            signaling_config,
            signaling_result_receiver,
            ..
        } => {
            match event {
                libp2p_swarm::handler::ConnectionEvent::FullyNegotiatedInbound(
                    fully_negotiated_inbound,
                ) => {
                    info!("Received connection event for fully negotiated inbound - Local peer id : {}", self.local_peer_id);

                    if role.is_some() {
                        tracing::info!("Ignoring inbound stream - role already assigned");
                        return;
                    }

                    if should_be_initiator {
                        tracing::warn!("Accepting inbound stream even though I should be initiator (race condition)");
                    }

                    tracing::info!(
                        "Negotiated inbound substream for signaling with peer {}",
                        self.peer
                    );

                    *role = Some(SignalingRole::Responder);
                    *signaling_status = SignalingStatus::Negotiating;

                    let substream = fully_negotiated_inbound.protocol;
                    let signaling_protocol = ProtocolHandler::new(signaling_config.clone());

                    let (tx, rx) = futures::channel::oneshot::channel();
                    *signaling_result_receiver = Some(rx);

                    wasm_bindgen_futures::spawn_local(async move {
                        tracing::debug!("Starting signaling as responder");
                        let signaling_result = signaling_protocol
                            .signaling_as_responder(SignalingStream::new(substream))
                            .await;

                        let _ = tx.send(signaling_result);
                    });
                }
                libp2p_swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(
                    fully_negotiated_outbound,
                ) => {
                    info!("Received conn event for fully negotiated outbound - Local peer id: {}", self.local_peer_id);

                    if role.is_some() {
                        tracing::info!("Ignoring outbound stream - role already assigned");
                        return;
                    }

                    if !should_be_initiator {
                        tracing::warn!("Opening outbound stream even though I should be responder (race condition)");
                    }

                    tracing::info!(
                        "Negotiated outbound substream for signaling with peer {}",
                        self.peer
                    );

                    *role = Some(SignalingRole::Initiator);
                    let substream = fully_negotiated_outbound.protocol;
                    let signaling_protocol = ProtocolHandler::new(signaling_config.clone());

                    let (tx, rx) = futures::channel::oneshot::channel();
                    *signaling_result_receiver = Some(rx);

                    wasm_bindgen_futures::spawn_local(async move {
                        tracing::info!("Starting signaling as initiator");
                        let signaling_result = signaling_protocol
                            .signaling_as_initiator(SignalingStream::new(substream))
                            .await;

                        let _ = tx.send(signaling_result);
                    });
                }
                libp2p_swarm::handler::ConnectionEvent::DialUpgradeError(error) => {
                    if *role == Some(SignalingRole::Initiator)
                        || *signaling_status == SignalingStatus::Negotiating
                    {
                        tracing::error!(
                            "Outbound signaling upgrade failed with peer {}: {:?}",
                            self.peer,
                            error
                        );
                        *signaling_status = SignalingStatus::Fail;
                    }
                }
                libp2p_swarm::handler::ConnectionEvent::ListenUpgradeError(error) => {
                    if *role == Some(SignalingRole::Responder)
                        || *signaling_status == SignalingStatus::Negotiating
                    {
                        tracing::error!(
                            "Inbound signaling upgrade failed with peer {}: {:?}",
                            self.peer,
                            error
                        );
                        *signaling_status = SignalingStatus::Fail;
                    }
                }
                _ => {}
            }
        }
    }
}
}
