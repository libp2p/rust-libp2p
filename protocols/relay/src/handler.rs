// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::protocol;
use crate::RequestId;
use futures::channel::oneshot::{self, Canceled};
use futures::future::BoxFuture;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use libp2p_core::either::{EitherError, EitherOutput};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use log::warn;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::io;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

/// Protocol handler that handles the relay protocol.
///
/// There are four possible situations in play here:
///
/// - The handler emits `RelayHandlerEvent::IncomingRelayRequest` if the node we handle asks us to
///   act as a relay. You must send a `RelayHandlerIn::OutgoingDestinationRequest` to another
///   handler, or send back a `DenyIncomingRelayRequest`.
///
/// - The handler emits `RelayHandlerEvent::IncomingDestinationRequest` if the node we handle asks
///   us to act as a destination. You must either call `accept` on the produced object, or send back
///   a `DenyDestinationRequest`.
///
/// - Send a `RelayHandlerIn::OutgoingRelayRequest` if the node we handle must act as a relay to a
///   destination. The handler will either send back a `RelayRequestSuccess` containing the stream
///   to the destination, or a `OutgoingRelayRequestDenied`.
///
/// - Send a `RelayHandlerIn::OutgoingDestinationRequest` if the node we handle must act as a
///   destination. The handler will automatically notify the source whether the request was accepted
///   or denied.
///
pub struct RelayHandler {
    /// Futures that send back negative responses.
    deny_futures: FuturesUnordered<BoxFuture<'static, Result<(), std::io::Error>>>,

    incoming_destination_request_pending_approval:
        HashMap<RequestId, protocol::IncomingDestinationRequest<NegotiatedSubstream>>,

    /// Futures that send back an accept response to a relay.
    accept_destination_futures: FuturesUnordered<
        BoxFuture<
            'static,
            Result<(PeerId, NegotiatedSubstream), protocol::IncomingDestinationRequestError>,
        >,
    >,

    /// Futures that copy from a source to a destination.
    copy_futures:
        FuturesUnordered<BoxFuture<'static, Result<(), protocol::IncomingRelayRequestError>>>,

    /// Requests asking the remote to become a relay.
    outgoing_relay_requests: Vec<OutgoingRelayRequest>,

    /// Requests asking the remote to become a destination.
    outgoing_destination_requests: SmallVec<
        [(
            PeerId,
            RequestId,
            Vec<Multiaddr>,
            protocol::IncomingRelayRequest<NegotiatedSubstream>,
        ); 4],
    >,

    /// Queue of events to return when polled.
    queued_events: Vec<RelayHandlerEvent>,

    /// Tracks substreams lend out to other [`Relayandler`]s.
    ///
    /// For each substream between a source and oneself, there is a future in here that resolves
    /// once the given substream is dropped.
    ///
    /// Once all substreams are dropped and this handler has no other work, [`KeepAlive::Until`] can
    /// be set eventually allowing the connection to be closed.
    alive_lend_out_substreams: FuturesUnordered<oneshot::Receiver<()>>,

    keep_alive: KeepAlive,
}

struct OutgoingRelayRequest {
    destination_peer_id: PeerId,
    request_id: RequestId,
    /// Addresses of the destination.
    destination_address: Multiaddr,
}

/// Event produced by the relay handler.
pub enum RelayHandlerEvent {
    /// The remote wants us to relay communications to a third party. You must either send back a
    /// `DenyIncomingRelayRequest`, or send a `OutgoingDestinationRequest` to a different handler containing
    /// this object.
    IncomingRelayRequest(
        RequestId,
        protocol::IncomingRelayRequest<NegotiatedSubstream>,
    ),

    /// The remote is a relay and is relaying a connection to us. In other words, we are used as
    /// destination. The behaviour can accept or deny the request via [`AcceptDestinationRequest`]
    /// or [`DenyDestinationRequest`].
    IncomingDestinationRequest(PeerId, RequestId),

    /// A `RelayRequest` that has previously been sent has been accepted by the remote. Contains
    /// a substream that communicates with the requested destination.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the destination. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    OutgoingRelayRequestSuccess(PeerId, RequestId, NegotiatedSubstream),

    /// The local node has accepted an incoming destination request. Contains a substream that
    /// communicates with the source.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the destination. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    IncomingRelayRequestSuccess {
        stream: NegotiatedSubstream,
        source: PeerId,
    },

    /// A `RelayRequest` that has previously been sent by the local node has failed.
    OutgoingRelayRequestError(PeerId, RequestId),
}

/// Event that can be sent to the relay handler.
pub enum RelayHandlerIn {
    /// Denies a hop request sent by the node we talk to.
    DenyIncomingRelayRequest(protocol::IncomingRelayRequest<NegotiatedSubstream>),

    /// Denies a destination request sent by the node we talk to.
    DenyDestinationRequest(PeerId, RequestId),

    /// Accepts a destination request sent by the node we talk to.
    AcceptDestinationRequest(PeerId, RequestId),

    /// Opens a new substream to the remote and asks it to relay communications to a third party.
    OutgoingRelayRequest {
        destination_peer_id: PeerId,
        request_id: RequestId,
        /// Addresses known for this peer to transmit to the remote.
        destination_address: Multiaddr,
    },

    /// Asks the node to be used as a destination for a relayed connection.
    ///
    /// The positive or negative response will be written to `substream`.
    OutgoingDestinationRequest {
        /// Peer id of the node whose communications are being relayed.
        source: PeerId,
        request_id: RequestId,
        /// Addresses of the node whose communications are being relayed.
        source_addresses: Vec<Multiaddr>,
        /// Substream to the source.
        substream: protocol::IncomingRelayRequest<NegotiatedSubstream>,
    },
}

impl RelayHandler {
    /// Builds a new `RelayHandler`.
    pub fn new() -> Self {
        RelayHandler {
            deny_futures: Default::default(),
            incoming_destination_request_pending_approval: Default::default(),
            accept_destination_futures: Default::default(),
            copy_futures: Default::default(),
            outgoing_relay_requests: Default::default(),
            outgoing_destination_requests: Default::default(),
            queued_events: Default::default(),
            alive_lend_out_substreams: Default::default(),
            keep_alive: KeepAlive::Yes,
        }
    }
}

impl Default for RelayHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolsHandler for RelayHandler {
    type InEvent = RelayHandlerIn;
    type OutEvent = RelayHandlerEvent;
    type Error = io::Error;
    type InboundProtocol = protocol::RelayListen;
    type OutboundProtocol = upgrade::EitherUpgrade<
        protocol::OutgoingRelayRequest,
        protocol::OutgoingDestinationRequest<protocol::IncomingRelayRequest<NegotiatedSubstream>>,
    >;
    type OutboundOpenInfo = (PeerId, RequestId);
    type InboundOpenInfo = RequestId;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::RelayListen::new(), RequestId::new())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as upgrade::InboundUpgrade<NegotiatedSubstream>>::Output,
        request_id: Self::InboundOpenInfo,
    ) {
        match protocol {
            // We have been asked to act as a relay.
            protocol::RelayRemoteRequest::RelayRequest((incoming_relay_request, notifyee)) => {
                self.alive_lend_out_substreams.push(notifyee);
                self.queued_events
                    .push(RelayHandlerEvent::IncomingRelayRequest(
                        request_id,
                        incoming_relay_request,
                    ));
            }
            // We have been asked to become a destination.
            protocol::RelayRemoteRequest::DestinationRequest(dest_request) => {
                let source = dest_request.source_id().clone();
                self.incoming_destination_request_pending_approval
                    .insert(request_id, dest_request);
                self.queued_events
                    .push(RelayHandlerEvent::IncomingDestinationRequest(
                        source, request_id,
                    ));
            }
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as upgrade::OutboundUpgrade<NegotiatedSubstream>>::Output,
        (destination_peer_id, request_id): Self::OutboundOpenInfo,
    ) {
        match protocol {
            // We have successfully negotiated a substream towards a relay.
            EitherOutput::First(substream_to_dest) => {
                self.queued_events
                    .push(RelayHandlerEvent::OutgoingRelayRequestSuccess(
                        destination_peer_id,
                        request_id,
                        substream_to_dest,
                    ));
            }
            // We have successfully asked the node to be a destination.
            EitherOutput::Second((to_dest_substream, request)) => {
                self.copy_futures.push(request.fulfill(to_dest_substream));
            }
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            // Deny a relay request from the node we handle.
            RelayHandlerIn::DenyIncomingRelayRequest(rq) => {
                let fut = rq.deny();
                self.deny_futures.push(fut);
            }
            RelayHandlerIn::AcceptDestinationRequest(_source, request_id) => {
                let rq = self
                    .incoming_destination_request_pending_approval
                    .remove(&request_id)
                    .unwrap();
                let fut = rq.accept();
                self.accept_destination_futures.push(fut);
            }
            // Deny a destination request from the node we handle.
            RelayHandlerIn::DenyDestinationRequest(_source, request_id) => {
                let rq = self
                    .incoming_destination_request_pending_approval
                    .remove(&request_id)
                    .unwrap();
                let fut = rq.deny();
                self.deny_futures.push(fut);
            }
            // Ask the node we handle to act as a relay.
            RelayHandlerIn::OutgoingRelayRequest {
                destination_peer_id,
                request_id,
                destination_address,
            } => {
                self.outgoing_relay_requests.push(OutgoingRelayRequest {
                    destination_peer_id,
                    request_id,
                    destination_address,
                });
            }
            // Ask the node we handle to act as a destination.
            RelayHandlerIn::OutgoingDestinationRequest {
                source,
                request_id,
                source_addresses,
                substream,
            } => {
                self.outgoing_destination_requests.push((
                    source,
                    request_id,
                    source_addresses,
                    substream,
                ));
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        (peer_id, request_id): Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<
            EitherError<
                protocol::OutgoingRelayRequestError,
                protocol::OutgoingDestinationRequestError,
            >,
        >,
    ) {
        match error {
            ProtocolsHandlerUpgrErr::Upgrade(upgrade::UpgradeError::Apply(EitherError::A(
                protocol::OutgoingRelayRequestError::Io(_),
            ))) => {
                self.queued_events
                    .push(RelayHandlerEvent::OutgoingRelayRequestError(
                        peer_id, request_id,
                    ));
            }
            // TODO: When the outbound destination request fails, send a status update back to the
            // source.
            _ => todo!(),
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        // Request the remote to act as a relay.
        if !self.outgoing_relay_requests.is_empty() {
            let OutgoingRelayRequest {
                destination_peer_id,
                request_id,
                destination_address,
            } = self.outgoing_relay_requests.remove(0);
            self.outgoing_relay_requests.shrink_to_fit();
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    upgrade::EitherUpgrade::A(protocol::OutgoingRelayRequest::new(
                        destination_peer_id,
                        destination_address,
                    )),
                    (destination_peer_id, request_id),
                ),
            });
        }

        // Request the remote to act as destination.
        if !self.outgoing_destination_requests.is_empty() {
            let (source, request_id, source_addrs, substream) =
                self.outgoing_destination_requests.remove(0);
            self.outgoing_destination_requests.shrink_to_fit();
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    upgrade::EitherUpgrade::B(protocol::OutgoingDestinationRequest::new(
                        source.clone(),
                        source_addrs,
                        substream,
                    )),
                    (source, request_id),
                ),
            });
        }

        match self.accept_destination_futures.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok((source, substream)))) => {
                let event = RelayHandlerEvent::IncomingRelayRequestSuccess {
                    stream: substream,
                    source,
                };
                return Poll::Ready(ProtocolsHandlerEvent::Custom(event));
            }
            Poll::Ready(Some(Err(e))) => panic!("{:?}", e),
            Poll::Ready(None) => {}
            Poll::Pending => {}
        }

        while let Poll::Ready(Some(result)) = self.copy_futures.poll_next_unpin(cx) {
            if let Err(e) = result {
                warn!("Incoming relay request failed: {:?}", e);
            }
        }

        while let Poll::Ready(Some(result)) = self.deny_futures.poll_next_unpin(cx) {
            if let Err(e) = result {
                warn!("Denying request failed: {:?}", e);
            }
        }

        // Report the queued events.
        if !self.queued_events.is_empty() {
            let event = self.queued_events.remove(0);
            return Poll::Ready(ProtocolsHandlerEvent::Custom(event));
        }

        while let Poll::Ready(Some(Err(Canceled))) =
            self.alive_lend_out_substreams.poll_next_unpin(cx)
        {}

        if !self.deny_futures.is_empty()
            || !self.accept_destination_futures.is_empty()
            || !self.copy_futures.is_empty()
            || !self.alive_lend_out_substreams.is_empty()
        {
            // Protocol handler is busy.
            self.keep_alive = KeepAlive::Yes;
        } else {
            // Protocol handler is idle.
            if matches!(self.keep_alive, KeepAlive::Yes) {
                self.keep_alive = KeepAlive::Until(Instant::now() + Duration::from_secs(2));
            }
        }

        Poll::Pending
    }
}
