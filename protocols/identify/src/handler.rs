// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use std::{
    collections::HashSet,
    task::{Context, Poll},
    time::Duration,
};

use either::Either;
use futures::prelude::*;
use futures_bounded::Timeout;
use futures_timer::Delay;
use libp2p_core::{
    upgrade::{ReadyUpgrade, SelectUpgrade},
    Multiaddr,
};
use libp2p_identity::{PeerId, PublicKey};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        ProtocolSupport,
    },
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol, SupportedProtocols,
};
use smallvec::SmallVec;
use tracing::Level;

use crate::{
    protocol,
    protocol::{Info, PushInfo, UpgradeError},
    PROTOCOL_NAME, PUSH_PROTOCOL_NAME,
};

const STREAM_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_CONCURRENT_STREAMS_PER_CONNECTION: usize = 10;

/// Protocol handler for sending and receiving identification requests.
///
/// Outbound requests are sent periodically. The handler performs expects
/// at least one identification request to be answered by the remote before
/// permitting the underlying connection to be closed.
pub struct Handler {
    remote_peer_id: PeerId,
    /// Pending events to yield.
    events: SmallVec<
        [ConnectionHandlerEvent<
            Either<ReadyUpgrade<StreamProtocol>, ReadyUpgrade<StreamProtocol>>,
            (),
            Event,
        >; 4],
    >,

    active_streams: futures_bounded::FuturesSet<Result<Success, UpgradeError>>,

    /// Future that fires when we need to identify the node again.
    trigger_next_identify: Delay,

    /// Whether we have exchanged at least one periodic identify.
    exchanged_one_periodic_identify: bool,

    /// The interval of `trigger_next_identify`, i.e. the recurrent delay.
    interval: Duration,

    /// The public key of the local peer.
    public_key: PublicKey,

    /// Application-specific version of the protocol family used by the peer,
    /// e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    protocol_version: String,

    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    agent_version: String,

    /// Address observed by or for the remote.
    observed_addr: Multiaddr,

    /// Identify information about the remote peer.
    remote_info: Option<Info>,

    local_supported_protocols: SupportedProtocols,
    remote_supported_protocols: HashSet<StreamProtocol>,
    external_addresses: HashSet<Multiaddr>,
}

/// An event from `Behaviour` with the information requested by the `Handler`.
#[derive(Debug)]
pub enum InEvent {
    AddressesChanged(HashSet<Multiaddr>),
    Push,
}

/// Event produced by the `Handler`.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Event {
    /// We obtained identification information from the remote.
    Identified(Info),
    /// We replied to an identification request from the remote.
    Identification,
    /// We actively pushed our identification information to the remote.
    IdentificationPushed(Info),
    /// Failed to identify the remote, or to reply to an identification request.
    IdentificationError(StreamUpgradeError<UpgradeError>),
}

impl Handler {
    /// Creates a new `Handler`.
    pub fn new(
        interval: Duration,
        remote_peer_id: PeerId,
        public_key: PublicKey,
        protocol_version: String,
        agent_version: String,
        observed_addr: Multiaddr,
        external_addresses: HashSet<Multiaddr>,
    ) -> Self {
        Self {
            remote_peer_id,
            events: SmallVec::new(),
            active_streams: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            trigger_next_identify: Delay::new(Duration::ZERO),
            exchanged_one_periodic_identify: false,
            interval,
            public_key,
            protocol_version,
            agent_version,
            observed_addr,
            local_supported_protocols: SupportedProtocols::default(),
            remote_supported_protocols: HashSet::default(),
            remote_info: Default::default(),
            external_addresses,
        }
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: output, ..
        }: FullyNegotiatedInbound<<Self as ConnectionHandler>::InboundProtocol>,
    ) {
        match output {
            future::Either::Left(stream) => {
                let info = self.build_info();

                if self
                    .active_streams
                    .try_push(
                        protocol::send_identify(stream, info).map_ok(|_| Success::SentIdentify),
                    )
                    .is_err()
                {
                    tracing::warn!("Dropping inbound stream because we are at capacity");
                } else {
                    self.exchanged_one_periodic_identify = true;
                }
            }
            future::Either::Right(stream) => {
                if self
                    .active_streams
                    .try_push(protocol::recv_push(stream).map_ok(Success::ReceivedIdentifyPush))
                    .is_err()
                {
                    tracing::warn!(
                        "Dropping inbound identify push stream because we are at capacity"
                    );
                }
            }
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: output, ..
        }: FullyNegotiatedOutbound<<Self as ConnectionHandler>::OutboundProtocol>,
    ) {
        match output {
            future::Either::Left(stream) => {
                if self
                    .active_streams
                    .try_push(protocol::recv_identify(stream).map_ok(Success::ReceivedIdentify))
                    .is_err()
                {
                    tracing::warn!("Dropping outbound identify stream because we are at capacity");
                }
            }
            future::Either::Right(stream) => {
                let info = self.build_info();

                if self
                    .active_streams
                    .try_push(
                        protocol::send_identify(stream, info).map_ok(Success::SentIdentifyPush),
                    )
                    .is_err()
                {
                    tracing::warn!(
                        "Dropping outbound identify push stream because we are at capacity"
                    );
                }
            }
        }
    }

    fn build_info(&mut self) -> Info {
        Info {
            public_key: self.public_key.clone(),
            protocol_version: self.protocol_version.clone(),
            agent_version: self.agent_version.clone(),
            listen_addrs: Vec::from_iter(self.external_addresses.iter().cloned()),
            protocols: Vec::from_iter(self.local_supported_protocols.iter().cloned()),
            observed_addr: self.observed_addr.clone(),
        }
    }

    /// If the public key matches the remote peer, handles the given `info` and returns `true`.
    fn handle_incoming_info(&mut self, info: &Info) -> bool {
        let derived_peer_id = info.public_key.to_peer_id();
        if self.remote_peer_id != derived_peer_id {
            return false;
        }

        self.remote_info.replace(info.clone());

        self.update_supported_protocols_for_remote(info);
        true
    }

    fn update_supported_protocols_for_remote(&mut self, remote_info: &Info) {
        let new_remote_protocols = HashSet::from_iter(remote_info.protocols.clone());

        let remote_added_protocols = new_remote_protocols
            .difference(&self.remote_supported_protocols)
            .cloned()
            .collect::<HashSet<_>>();
        let remote_removed_protocols = self
            .remote_supported_protocols
            .difference(&new_remote_protocols)
            .cloned()
            .collect::<HashSet<_>>();

        if !remote_added_protocols.is_empty() {
            self.events
                .push(ConnectionHandlerEvent::ReportRemoteProtocols(
                    ProtocolSupport::Added(remote_added_protocols),
                ));
        }

        if !remote_removed_protocols.is_empty() {
            self.events
                .push(ConnectionHandlerEvent::ReportRemoteProtocols(
                    ProtocolSupport::Removed(remote_removed_protocols),
                ));
        }

        self.remote_supported_protocols = new_remote_protocols;
    }

    fn local_protocols_to_string(&mut self) -> String {
        self.local_supported_protocols
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = InEvent;
    type ToBehaviour = Event;
    type InboundProtocol =
        SelectUpgrade<ReadyUpgrade<StreamProtocol>, ReadyUpgrade<StreamProtocol>>;
    type OutboundProtocol = Either<ReadyUpgrade<StreamProtocol>, ReadyUpgrade<StreamProtocol>>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(
            SelectUpgrade::new(
                ReadyUpgrade::new(PROTOCOL_NAME),
                ReadyUpgrade::new(PUSH_PROTOCOL_NAME),
            ),
            (),
        )
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            InEvent::AddressesChanged(addresses) => {
                self.external_addresses = addresses;
            }
            InEvent::Push => {
                self.events
                    .push(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            Either::Right(ReadyUpgrade::new(PUSH_PROTOCOL_NAME)),
                            (),
                        ),
                    });
            }
        }
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Event>> {
        if let Some(event) = self.events.pop() {
            return Poll::Ready(event);
        }

        // Poll the future that fires when we need to identify the node again.
        if let Poll::Ready(()) = self.trigger_next_identify.poll_unpin(cx) {
            self.trigger_next_identify.reset(self.interval);
            let event = ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    Either::Left(ReadyUpgrade::new(PROTOCOL_NAME)),
                    (),
                ),
            };
            return Poll::Ready(event);
        }

        while let Poll::Ready(ready) = self.active_streams.poll_unpin(cx) {
            match ready {
                Ok(Ok(Success::ReceivedIdentify(remote_info))) => {
                    if self.handle_incoming_info(&remote_info) {
                        return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                            Event::Identified(remote_info),
                        ));
                    } else {
                        tracing::warn!(
                            %self.remote_peer_id,
                            ?remote_info.public_key,
                            derived_peer_id=%remote_info.public_key.to_peer_id(),
                            "Discarding received identify message as public key does not match remote peer ID",
                        );
                    }
                }
                Ok(Ok(Success::SentIdentifyPush(info))) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::IdentificationPushed(info),
                    ));
                }
                Ok(Ok(Success::SentIdentify)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::Identification,
                    ));
                }
                Ok(Ok(Success::ReceivedIdentifyPush(remote_push_info))) => {
                    if let Some(mut info) = self.remote_info.clone() {
                        info.merge(remote_push_info);

                        if self.handle_incoming_info(&info) {
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                                Event::Identified(info),
                            ));
                        } else {
                            tracing::warn!(
                                %self.remote_peer_id,
                                ?info.public_key,
                                derived_peer_id=%info.public_key.to_peer_id(),
                                "Discarding received identify message as public key does not match remote peer ID",
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::IdentificationError(StreamUpgradeError::Apply(e)),
                    ));
                }
                Err(Timeout { .. }) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::IdentificationError(StreamUpgradeError::Timeout),
                    ));
                }
            }
        }

        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, .. }) => {
                self.events.push(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(
                        error.map_upgrade_err(|e| libp2p_core::util::unreachable(e.into_inner())),
                    ),
                ));
                self.trigger_next_identify.reset(self.interval);
            }
            ConnectionEvent::LocalProtocolsChange(change) => {
                let before = tracing::enabled!(Level::DEBUG)
                    .then(|| self.local_protocols_to_string())
                    .unwrap_or_default();
                let protocols_changed = self.local_supported_protocols.on_protocols_change(change);
                let after = tracing::enabled!(Level::DEBUG)
                    .then(|| self.local_protocols_to_string())
                    .unwrap_or_default();

                if protocols_changed && self.exchanged_one_periodic_identify {
                    tracing::debug!(
                        peer=%self.remote_peer_id,
                        %before,
                        %after,
                        "Supported listen protocols changed, pushing to peer"
                    );

                    self.events
                        .push(ConnectionHandlerEvent::OutboundSubstreamRequest {
                            protocol: SubstreamProtocol::new(
                                Either::Right(ReadyUpgrade::new(PUSH_PROTOCOL_NAME)),
                                (),
                            ),
                        });
                }
            }
            _ => {}
        }
    }
}

enum Success {
    SentIdentify,
    ReceivedIdentify(Info),
    SentIdentifyPush(Info),
    ReceivedIdentifyPush(PushInfo),
}
