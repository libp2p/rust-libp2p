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

use crate::protocol::{recv_identify, recv_push, send, UpgradeError};
use crate::protocol::{Info, PushInfo};
use crate::{PROTOCOL_NAME, PUSH_PROTOCOL_NAME};
use either::Either;
use futures::future::BoxFuture;
use futures::prelude::*;
use futures_bounded::Timeout;
use futures_timer::Delay;
use libp2p_core::upgrade::{ReadyUpgrade, SelectUpgrade};
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_identity::PublicKey;
use libp2p_swarm::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
    ProtocolSupport,
};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, KeepAlive, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol, SupportedProtocols,
};
use log::{warn, Level};
use smallvec::SmallVec;
use std::collections::HashSet;
use std::{io, task::Context, task::Poll, time::Duration};

const STREAM_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_CONCURRENT_STREAMS_PER_CONNECTION: usize = 10;

/// Protocol handler for sending and receiving identification requests.
///
/// Outbound requests are sent periodically. The handler performs expects
/// at least one identification request to be answered by the remote before
/// permitting the underlying connection to be closed.
pub struct Handler {
    remote_peer_id: PeerId,
    inbound_identify_push: Option<BoxFuture<'static, Result<PushInfo, UpgradeError>>>,
    /// Pending events to yield.
    events: SmallVec<
        [ConnectionHandlerEvent<
            Either<ReadyUpgrade<StreamProtocol>, ReadyUpgrade<StreamProtocol>>,
            (),
            Event,
            io::Error,
        >; 4],
    >,

    /// Pending identification replies, awaiting being sent.
    pending_replies: futures_bounded::FuturesSet<Result<(), UpgradeError>>,

    /// Pending identify requests.
    outbound_identify_futs: futures_bounded::FuturesSet<Result<Info, UpgradeError>>,

    /// Pending identify/push requests.
    outbound_identify_push_futs: futures_bounded::FuturesSet<Result<(), UpgradeError>>,

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
    IdentificationPushed,
    /// Failed to identify the remote, or to reply to an identification request.
    IdentificationError(StreamUpgradeError<UpgradeError>),
}

impl Handler {
    /// Creates a new `Handler`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        initial_delay: Duration,
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
            inbound_identify_push: Default::default(),
            events: SmallVec::new(),
            pending_replies: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            outbound_identify_futs: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            outbound_identify_push_futs: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            trigger_next_identify: Delay::new(initial_delay),
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
        }: FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
        match output {
            future::Either::Left(stream) => {
                let info = self.build_info();

                if self
                    .pending_replies
                    .try_push(crate::protocol::send(stream, info))
                    .is_err()
                {
                    warn!("Dropping inbound stream because we are at capacity");
                }
            }
            future::Either::Right(stream) => {
                if self
                    .inbound_identify_push
                    .replace(recv_push(stream).boxed())
                    .is_some()
                {
                    warn!(
                        "New inbound identify push stream from {} while still \
                         upgrading previous one. Replacing previous with new.",
                        self.remote_peer_id,
                    );
                }
            }
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: output, ..
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        match output {
            future::Either::Left(stream) => {
                if self
                    .outbound_identify_futs
                    .try_push(recv_identify(stream))
                    .is_err()
                {
                    warn!("Dropping outbound identify stream because we are at capacity");
                }
            }
            future::Either::Right(stream) => {
                let info = self.build_info();

                if self
                    .outbound_identify_push_futs
                    .try_push(send(stream, info))
                    .is_err()
                {
                    warn!("Dropping outbound identify push stream because we are at capacity");
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

    fn handle_incoming_info(&mut self, info: &Info) {
        self.remote_info.replace(info.clone());

        self.update_supported_protocols_for_remote(info);
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
    type Error = io::Error;
    type InboundProtocol =
        SelectUpgrade<ReadyUpgrade<StreamProtocol>, ReadyUpgrade<StreamProtocol>>;
    type OutboundProtocol = Either<ReadyUpgrade<StreamProtocol>, ReadyUpgrade<StreamProtocol>>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
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

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.inbound_identify_push.is_some() {
            return KeepAlive::Yes;
        }

        if !self.pending_replies.is_empty() {
            return KeepAlive::Yes;
        }

        KeepAlive::No
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Event, Self::Error>,
    > {
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

        if let Some(Poll::Ready(res)) = self
            .inbound_identify_push
            .as_mut()
            .map(|f| f.poll_unpin(cx))
        {
            self.inbound_identify_push.take();

            if let Ok(remote_push_info) = res {
                if let Some(mut info) = self.remote_info.clone() {
                    info.merge(remote_push_info);
                    self.handle_incoming_info(&info);

                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::Identified(info),
                    ));
                };
            }
        }

        // Check for pending replies.
        match self.pending_replies.poll_unpin(cx) {
            Poll::Ready(Ok(Ok(()))) => {
                self.exchanged_one_periodic_identify = true;

                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::Identification,
                ));
            }
            Poll::Ready(Ok(Err(e))) => {
                self.exchanged_one_periodic_identify = true;

                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(StreamUpgradeError::Apply(e)),
                ));
            }
            Poll::Ready(Err(Timeout { .. })) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(StreamUpgradeError::Timeout),
                ));
            }
            Poll::Pending => {}
        }

        match self.outbound_identify_futs.poll_unpin(cx) {
            Poll::Ready(Ok(Ok(remote_info))) => {
                self.handle_incoming_info(&remote_info);

                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::Identified(
                    remote_info,
                )));
            }
            Poll::Ready(Ok(Err(e))) => {
                self.trigger_next_identify.reset(self.interval);

                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(StreamUpgradeError::Apply(e)),
                ));
            }
            Poll::Ready(Err(Timeout { .. })) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(StreamUpgradeError::Timeout),
                ));
            }
            Poll::Pending => {}
        }

        match self.outbound_identify_push_futs.poll_unpin(cx) {
            Poll::Ready(Ok(Ok(()))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationPushed,
                ));
            }
            Poll::Ready(Ok(Err(e))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(StreamUpgradeError::Apply(e)),
                ));
            }
            Poll::Ready(Err(Timeout { .. })) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(StreamUpgradeError::Timeout),
                ));
            }
            Poll::Pending => {}
        }

        Poll::Pending
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
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, .. }) => {
                let upgrade_error = match error {
                    StreamUpgradeError::Timeout => StreamUpgradeError::Timeout,
                    StreamUpgradeError::NegotiationFailed => StreamUpgradeError::NegotiationFailed,
                    StreamUpgradeError::Io(e) => StreamUpgradeError::Io(e),
                    StreamUpgradeError::Apply(v) => unreachable!("{}", v),
                };

                self.events.push(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::IdentificationError(upgrade_error),
                ));
                self.trigger_next_identify.reset(self.interval);
            }
            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::ListenUpgradeError(_)
            | ConnectionEvent::RemoteProtocolsChange(_) => {}
            ConnectionEvent::LocalProtocolsChange(change) => {
                let before = log::log_enabled!(Level::Debug)
                    .then(|| self.local_protocols_to_string())
                    .unwrap_or_default();
                let protocols_changed = self.local_supported_protocols.on_protocols_change(change);
                let after = log::log_enabled!(Level::Debug)
                    .then(|| self.local_protocols_to_string())
                    .unwrap_or_default();

                if protocols_changed && self.exchanged_one_periodic_identify {
                    log::debug!(
                        "Supported listen protocols changed from [{before}] to [{after}], pushing to {}",
                        self.remote_peer_id
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
        }
    }
}
