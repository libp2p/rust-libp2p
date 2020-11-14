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
use futures::future::BoxFuture;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use libp2p_core::{either, upgrade, Multiaddr, PeerId};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use smallvec::SmallVec;
use std::task::{Context, Poll};
use std::{error, io};

/// Protocol handler that handles the relay protocol.
///
/// There are four possible situations in play here:
///
/// - The handler emits `RelayHandlerEvent::IncomingRelayRequest` if the node we handle asks us to act as a
///   relay. You must send a `RelayHandlerIn::OutgoingDestinationRequest` to another handler, or send back
///   a `DenyIncomingRelayRequest`.
///
/// - The handler emits `RelayHandlerEvent::IncomingDestinationRequest` if the node we handle asks us to
///   act as a destination. You must either call `accept` on the produced object, or send back a
///   `DenyDestinationRequest`.
///
/// - Send a `RelayHandlerIn::OutgoingRelayRequest` if the node we handle must act as a relay to a
///   destination. The handler will either send back a `RelayRequestSuccess` containing the
///   stream to the destination, or a `OutgoingRelayRequestDenied`.
///
/// - Send a `RelayHandlerIn::OutgoingDestinationRequest` if the node we handle must act as a destination.
///   The handler will automatically notify the source whether the request was accepted or denied.
///
pub struct RelayHandler {
    /// Futures that send back negative responses.
    // TODO: Use FuturesUnordered here and below.
    deny_futures: SmallVec<[BoxFuture<'static, Result<(), io::Error>>; 4]>,

    /// Futures that send back an accept response to a relay.
    accept_destination_futures: FuturesUnordered<
        BoxFuture<'static, Result<(PeerId, NegotiatedSubstream), Box<dyn error::Error + 'static>>>,
    >,

    /// Futures that copy from a source to a destination.
    copy_futures: FuturesUnordered<BoxFuture<'static, ()>>,

    /// Requests asking the remote to become a relay.
    outgoing_relay_requests: SmallVec<[(PeerId, Vec<Multiaddr>); 4]>,

    /// Requests asking the remote to become a destination.
    outgoing_destination_requests: SmallVec<[(PeerId, Vec<Multiaddr>, RelayHandlerIncomingRelayRequest); 4]>,

    /// Queue of events to return when polled.
    queued_events: Vec<RelayHandlerEvent>,
}

/// Event produced by the relay handler.
//#[derive(Debug)]      // TODO: restore
pub enum RelayHandlerEvent {
    /// The remote wants us to relay communications to a third party. You must either send back a
    /// `DenyIncomingRelayRequest`, or send a `OutgoingDestinationRequest` to a different handler containing
    /// this object.
    IncomingRelayRequest(RelayHandlerIncomingRelayRequest),

    /// The remote is a relay and is relaying a connection to us. In other words, we are used as
    /// destination. You must either call `accept` on the object, or send back a
    /// `DenyDestinationRequest` to the handler.
    IncomingDestinationRequest(RelayHandlerIncomingDestinationRequest),

    /// A `RelayRequest` that has previously been sent has been accepted by the remote. Contains
    /// a substream that communicates with the requested destination.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the destination. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    OutgoingRelayRequestSuccess(PeerId, NegotiatedSubstream),

    /// The local node has accepted an incoming destination request. Contains a substream that
    /// communicates with the source.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the destination. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    IncomingRelayRequestSuccess{
        stream: NegotiatedSubstream,
        source: PeerId,
    },

    /// A `RelayRequest` that has previously been sent by the local node has been denied by the
    /// remote.
    OutgoingRelayRequestDenied(PeerId),
}

/// Event that can be sent to the relay handler.
#[derive(Clone)]
pub enum RelayHandlerIn {
    /// Denies a hop request sent by the node we talk to.
    DenyIncomingRelayRequest(RelayHandlerIncomingRelayRequest),

    /// Denies a destination request sent by the node we talk to.
    DenyDestinationRequest(RelayHandlerIncomingDestinationRequest),

    AcceptDestinationRequest(RelayHandlerIncomingDestinationRequest),

    /// Opens a new substream to the remote and asks it to relay communications to a third party.
    OutgoingRelayRequest {
        /// Id of the peer to connect to.
        target: PeerId,
        /// Addresses known for this peer to transmit to the remote.
        addresses: Vec<Multiaddr>,
    },

    /// Asks the node to be used as a destination for a relayed connection.
    ///
    /// The positive or negative response will be written to `substream`.
    OutgoingDestinationRequest {
        /// Peer id of the node whose communications are being relayed.
        source: PeerId,
        /// Addresses of the node whose communications are being relayed.
        source_addresses: Vec<Multiaddr>,
        /// Substream to the source.
        substream: RelayHandlerIncomingRelayRequest,
    },
}

/// The remote wants us to become a relay.
#[derive(Clone)]
pub struct RelayHandlerIncomingRelayRequest {
    inner: protocol::IncomingRelayRequest<NegotiatedSubstream>,
}

impl RelayHandlerIncomingRelayRequest {
    /// Peer id of the destination that we should relay to.
    pub fn destination_id(&self) -> &PeerId {
        self.inner.destination_id()
    }

    /// Returns the addresses of the target, as reported by the requester.
    pub fn destination_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.inner.destination_addresses()
    }
}

/// The remote wants us to be treated as a destination.
#[derive(Clone)]
pub struct RelayHandlerIncomingDestinationRequest {
    inner: protocol::IncomingDestinationRequest<NegotiatedSubstream>,
}

impl RelayHandlerIncomingDestinationRequest {
    /// Peer id of the source that is being relayed.
    pub fn source_id(&self) -> &PeerId {
        self.inner.source_id()
    }

    /// Addresses of the source that is being relayed.
    pub fn source_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.inner.source_addresses()
    }

    /// Accepts the request. Produces a `Future` that sends back a success message then provides
    /// the stream to the source.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the source. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    // TODO: change error type
    pub fn accept(
        self,
    ) -> impl Future<Output = Result<(PeerId, NegotiatedSubstream), Box<dyn error::Error + 'static>>> {
        self.inner.accept()
    }
}

impl RelayHandler {
    /// Builds a new `RelayHandler`.
    pub fn new() -> Self {
        RelayHandler {
            deny_futures: Default::default(),
            accept_destination_futures: Default::default(),
            copy_futures: Default::default(),
            outgoing_relay_requests: Default::default(),
            outgoing_destination_requests: Default::default(),
            queued_events: Default::default(),
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
        protocol::OutgoingRelayRequest<PeerId>,
        protocol::OutgoingDestinationRequest<RelayHandlerIncomingRelayRequest>,
    >;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::RelayListen::new(), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as upgrade::InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
        match protocol {
            // We have been asked to become a destination.
            protocol::RelayRemoteRequest::DestinationRequest(dest_request) => {
                self.queued_events
                    .push(RelayHandlerEvent::IncomingDestinationRequest(
                        RelayHandlerIncomingDestinationRequest {
                            inner: dest_request,
                        },
                    ));
            }
            // We have been asked to act as a relay.
            protocol::RelayRemoteRequest::HopRequest(hop_request) => {
                self.queued_events
                    .push(RelayHandlerEvent::IncomingRelayRequest(RelayHandlerIncomingRelayRequest {
                        inner: hop_request,
                    }));
            }
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as upgrade::OutboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        match protocol {
            // We have successfully negotiated a substream towards a destination.
            either::EitherOutput::First((substream_to_dest, dest_id)) => {
                self.queued_events
                    .push(RelayHandlerEvent::OutgoingRelayRequestSuccess(
                        dest_id,
                        substream_to_dest,
                    ));
            }
            // We have successfully asked the node to be a destination.
            either::EitherOutput::Second((to_dest_substream, hop_request)) => {
                self.copy_futures.push(hop_request.inner.fulfill(to_dest_substream));
            }
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            // Deny a relay request from the node we handle.
            RelayHandlerIn::DenyIncomingRelayRequest(rq) => {
                let fut = rq.inner.deny();
                self.deny_futures.push(fut);
            }
            RelayHandlerIn::AcceptDestinationRequest(rq) => {
                let fut = rq.inner.accept();
                self.accept_destination_futures.push(fut.boxed());
            }
            // Deny a destination request from the node we handle.
            RelayHandlerIn::DenyDestinationRequest(rq) => {
                let fut = rq.inner.deny();
                self.deny_futures.push(fut);
            }
            // Ask the node we handle to act as a relay.
            RelayHandlerIn::OutgoingRelayRequest { target, addresses } => {
                self.outgoing_relay_requests
                    .push((target.clone(), addresses.clone()));
            }
            // Ask the node we handle to act as a destination.
            RelayHandlerIn::OutgoingDestinationRequest {
                source,
                source_addresses,
                substream,
            } => {
                self.outgoing_destination_requests
                    .push((source, source_addresses, substream));
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        // TODO: Fix
        _: ProtocolsHandlerUpgrErr<
            either::EitherError<protocol::SendReadError, protocol::SendReadError>,
        >,
    ) {
        // TODO:
        unimplemented!()
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        // TODO: Rework this.
        //
        // TODO: In case the local node acts as a relay, and this handler is the handler to the source,
        // make sure this handler is shut down once the last copy future copying between source and
        // destination and vice versa is done.
        KeepAlive::Yes
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
            let (peer_id, addrs) = self.outgoing_relay_requests.remove(0);
            self.outgoing_relay_requests.shrink_to_fit();
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    upgrade::EitherUpgrade::A(protocol::OutgoingRelayRequest::new(
                        peer_id.clone(),
                        addrs,
                        peer_id,
                    )),
                    (),
                ),
            });
        }

        // Request the remote to act as destination.
        if !self.outgoing_destination_requests.is_empty() {
            let (source, source_addrs, substream) = self.outgoing_destination_requests.remove(0);
            self.outgoing_destination_requests.shrink_to_fit();
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    upgrade::EitherUpgrade::B(protocol::OutgoingDestinationRequest::new(
                        source,
                        source_addrs,
                        substream,
                    )),
                    (),
                ),
            });
        }

        match self.accept_destination_futures.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok((source, substream)))) => {
                let event = RelayHandlerEvent::IncomingRelayRequestSuccess{
                    stream: substream,
                    source,
                };
                return Poll::Ready(ProtocolsHandlerEvent::Custom(event));
            }
            Poll::Ready(Some(Err(e))) => panic!("{:?}", e),
            Poll::Ready(None) => {}
            Poll::Pending => {}
        }

        match self.copy_futures.poll_next_unpin(cx) {
            Poll::Ready(Some(())) => panic!("a copy future finished"),
            Poll::Ready(None) => {},
            Poll::Pending => {},
        }

        // Report the queued events.
        if !self.queued_events.is_empty() {
            let event = self.queued_events.remove(0);
            return Poll::Ready(ProtocolsHandlerEvent::Custom(event));
        }

        Poll::Pending
    }
}
