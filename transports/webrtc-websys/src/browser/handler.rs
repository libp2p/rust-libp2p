use std::task::Poll;

use futures::FutureExt;
use libp2p_core::{upgrade::ReadyUpgrade, PeerId};
use libp2p_swarm::{ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol};
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
    local_peer_id: PeerId,
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
    // A noop flag disabling connection handling abilities for this handler
    is_noop: bool
}

impl SignalingHandler {
    pub fn new(
        local_peer_id: PeerId,
        peer: PeerId,
        is_dialer: bool,
        config: SignalingConfig,
        is_noop: bool
    ) -> Self {
        Self {
            role: None,
            peer,
            is_dialer,
            retry_count: 0,
            signaling_config: config,
            signaling_status: SignalingStatus::Idle,
            signaling_result_receiver: None,
            local_peer_id,
            is_noop
        }
    }

    /// Check if signaling should be retried based on the error and current state
    fn should_retry(&self, _error: &crate::Error) -> bool {
        self.role == Some(SignalingRole::Initiator)
            && self.retry_count < self.signaling_config.max_signaling_retries
    }

    /// Reset handler state for retry
    fn reset_for_retry(&mut self) {
        self.signaling_status = SignalingStatus::WaitingForRetry;
        self.retry_count += 1;
        self.role = None;
        self.signaling_result_receiver = None;
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
        self.signaling_status = SignalingStatus::Complete;

        self.signaling_result_receiver = None;

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
            tracing::info!(
                "Retrying signaling attempt {} of {} with peer {}",
                self.retry_count + 1,
                self.signaling_config.max_signaling_retries,
                self.peer
            );

            self.reset_for_retry();

            ConnectionHandlerEvent::NotifyBehaviour(ToBehaviourEvent::SignalingRetry)
        } else {
            tracing::error!("WebRTC signaling failed permanently: {:?}", error);
            self.signaling_status = SignalingStatus::Fail;
            ConnectionHandlerEvent::NotifyBehaviour(ToBehaviourEvent::WebRTCConnectionFailure(
                error,
            ))
        }
    }
}

impl SignalingHandler {
    fn should_be_initiator(&self, remote_peer: &PeerId) -> bool {
        // Use lexicographic comparison of peer IDs for deterministic role assignment
        self.local_peer_id < *remote_peer
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
        if self.is_noop {
            return Poll::Pending;
        }

        if self.signaling_status == SignalingStatus::Complete {
            return Poll::Pending;
        }

        if let Some(mut receiver) = self.signaling_result_receiver.take() {
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
                    // Put the signaling result channel back to poll again later
                    self.signaling_result_receiver = Some(receiver);
                }
            }
        }

        // Check the signaling status for this handler for any pending outbound signaling
        // attempts
        if self.signaling_status == SignalingStatus::AwaitingInitiation {
            self.signaling_status = SignalingStatus::Negotiating;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(SIGNALING_STREAM_PROTOCOL), ()),
            });
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        if self.is_noop {
            return;
        }

        match event {
            FromBehaviourEvent::InitiateSignaling => {
                if self.signaling_status == SignalingStatus::Idle
                    || self.signaling_status == SignalingStatus::WaitingForRetry
                {
                    // Only initiate if we should be the initiator
                    if self.should_be_initiator(&self.peer) {
                        tracing::info!("Initiating signaling with peer {}", self.peer);
                        self.signaling_status = SignalingStatus::AwaitingInitiation;
                    } else {
                        tracing::info!("Waiting for peer {} to initiate signaling", self.peer);
                    }
                }
            }
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
        if self.is_noop {
            return;
        }

        match event {
            libp2p_swarm::handler::ConnectionEvent::FullyNegotiatedInbound(
                fully_negotiated_inbound,
            ) => {
                info!("Received connection event for fully negotiated inbound - Local peer id : {}", self.local_peer_id);
                // Only handle inbound if we haven't already established a role AND we should be responder
                if self.role.is_some() || self.should_be_initiator(&self.peer) {
                    tracing::info!(
                        "Ignoring inbound stream - role already assigned or should be initiator"
                    );
                    return;
                }

                tracing::info!(
                    "Negotiated inbound substream for signaling with peer {}",
                    self.peer
                );

                self.role = Some(SignalingRole::Responder);
                self.signaling_status = SignalingStatus::Negotiating;

                let substream = fully_negotiated_inbound.protocol;
                let signaling_protocol = ProtocolHandler::new(self.signaling_config.clone());

                let (tx, rx) = futures::channel::oneshot::channel();
                self.signaling_result_receiver = Some(rx);

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
                // Only handle outbound if we haven't already established a role
                if self.role.is_some() || !self.should_be_initiator(&self.peer) {
                    tracing::info!(
                        "Ignoring outbound stream - role already assigned or should be responder"
                    );
                    return;
                }

                tracing::info!(
                    "Negotiated outbound substream for signaling with peer {}",
                    self.peer
                );

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
            libp2p_swarm::handler::ConnectionEvent::DialUpgradeError(error) => {
                if self.role == Some(SignalingRole::Initiator)
                    || self.signaling_status == SignalingStatus::Negotiating
                {
                    tracing::error!(
                        "Outbound signaling upgrade failed with peer {}: {:?}",
                        self.peer,
                        error
                    );
                    self.signaling_status = SignalingStatus::Fail;
                }
            }
            libp2p_swarm::handler::ConnectionEvent::ListenUpgradeError(error) => {
                if self.role == Some(SignalingRole::Responder)
                    || self.signaling_status == SignalingStatus::Negotiating
                {
                    tracing::error!(
                        "Inbound signaling upgrade failed with peer {}: {:?}",
                        self.peer,
                        error
                    );
                    self.signaling_status = SignalingStatus::Fail;
                }
            }
            _ => {}
        }
    }
}
