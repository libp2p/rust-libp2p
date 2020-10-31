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

use crate::copy::Copy;
use crate::protocol;
use futures::prelude::*;
use futures::future::BoxFuture;
use libp2p_swarm::{
    KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol,
    NegotiatedSubstream,
};
use libp2p_core::{either, upgrade, upgrade::OutboundUpgrade, Multiaddr, PeerId};
use smallvec::SmallVec;
use std::{error, io};
use std::task::{Context, Poll};

/// Protocol handler that handles the relay protocol.
///
/// There are four possible situations in play here:
///
/// - The handler emits `RelayHandlerEvent::HopRequest` if the node we handle asks us to act as a
///   relay. You must send a `RelayHandlerIn::DestinationRequest` to another handler, or send back
///   a `DenyHopRequest`.
///
/// - The handler emits `RelayHandlerEvent::DestinationRequest` if the node we handle asks us to
///   act as a destination. You must either call `accept` on the produced object, or send back a
///   `DenyDestinationRequest`.
///
/// - Send a `RelayHandlerIn::RelayRequest` if the node we handle must act as a relay to a
///   destination. The handler will either send back a `RelayRequestSuccess` containing the
///   stream to the destination, or a `RelayRequestDenied`.
///
/// - Send a `RelayHandlerIn::DestinationRequest` if the node we handle must act as a destination.
///   The handler will automatically notify the source whether the request was accepted or denied.
///
pub struct RelayHandler {
    /// Futures that send back negative responses.
    deny_futures: SmallVec<[BoxFuture<'static, Result<(), io::Error>>; 4]>,

    /// Futures that send back an accept response to a source.
    accept_futures: SmallVec<[protocol::RelayHopAcceptFuture<NegotiatedSubstream, NegotiatedSubstream>; 4]>,

    /// Futures that copy from a source to a destination.
    copy_futures: SmallVec<[Copy<NegotiatedSubstream, NegotiatedSubstream>; 4]>,

    /// List of requests to relay we should send to the node we handle.
    relay_requests: SmallVec<[(PeerId, Vec<Multiaddr>); 4]>,

    /// List of requests to be a destination we should send to the node we handle.
    dest_requests: SmallVec<[(PeerId, Vec<Multiaddr>, RelayHandlerHopRequest); 4]>,

    /// Queue of events to return when polled.
    queued_events: Vec<RelayHandlerEvent>,
}

/// Event produced by the relay handler.
//#[derive(Debug)]      // TODO: restore
pub enum RelayHandlerEvent {
    /// The remote wants us to relay communications to a third party. You must either send back
    /// a `DenyHopRequest`, or send a `DestinationRequest` to a different handler containing this
    /// object.
    HopRequest(RelayHandlerHopRequest),

    /// The remote is a relay and is relaying a connection to us. In other words, we are used as
    /// destination. You must either call `accept` on the object, or send back a
    /// `DenyDestinationRequest` to the handler.
    DestinationRequest(RelayHandlerDestRequest),

    /// A `RelayRequest` that has previously been sent has been accepted by the remote. Contains
    /// a substream that communicates with the requested destination.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the destination. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    RelayRequestSuccess(PeerId, NegotiatedSubstream),

    /// A `RelayRequest` that has previously been sent has been denied by the remote. Contains
    /// a substream that communicates with the requested destination.
    RelayRequestDenied(PeerId),
}

/// Event that can be sent to the relay handler.
#[derive(Clone)]
pub enum RelayHandlerIn {
    /// Denies a hop request sent by the node we talk to.
    DenyHopRequest(RelayHandlerHopRequest),

    /// Denies a destination request sent by the node we talk to.
    DenyDestinationRequest(RelayHandlerDestRequest),

    /// Opens a new substream to the remote and asks it to relay communications to a third party.
    RelayRequest {
        /// Id of the peer to connect to.
        target: PeerId,
        /// Addresses known for this peer to transmit to the remote.
        addresses: Vec<Multiaddr>,
    },

    /// Asks the node to be used as a destination for a relayed connection.
    ///
    /// The positive or negative response will be written to `substream`.
    DestinationRequest {
        /// Peer id of the node whose communications are being relayed.
        source: PeerId,
        /// Addresses of the node whose communications are being relayed.
        source_addresses: Vec<Multiaddr>,
        /// Substream to the source.
        substream: RelayHandlerHopRequest,
    },
}

/// The remote wants us to become a relay.
#[derive(Clone)]
pub struct RelayHandlerHopRequest {
    inner: protocol::RelayHopRequest<NegotiatedSubstream>,
}

impl RelayHandlerHopRequest {
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
pub struct RelayHandlerDestRequest {
    inner: protocol::RelayDestinationRequest<NegotiatedSubstream>,
}

impl RelayHandlerDestRequest {
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
    ) -> impl Future<Output = Result<NegotiatedSubstream, Box<dyn error::Error + 'static>>>
    {
        self.inner.accept()
    }
}

impl RelayHandler {
    /// Builds a new `RelayHandler`.
    pub fn new() -> Self {
        RelayHandler {
            deny_futures: SmallVec::new(),
            accept_futures: SmallVec::new(),
            copy_futures: SmallVec::new(),
            relay_requests: SmallVec::new(),
            dest_requests: SmallVec::new(),
            queued_events: Vec::new(),
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
        protocol::RelayProxyRequest<PeerId>,
        protocol::RelayTargetOpen<RelayHandlerHopRequest>,
    >;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        println!("RelayHandler::listen_protocol");
        SubstreamProtocol::new(protocol::RelayListen::new(), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as upgrade::InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
        println!("RelayHandler::inject_fully_negotiated_inbound");
        match protocol {
            // We have been asked to become a destination.
            protocol::RelayRemoteRequest::DestinationRequest(dest_request) => {
                self.queued_events
                    .push(RelayHandlerEvent::DestinationRequest(
                        RelayHandlerDestRequest {
                            inner: dest_request,
                        },
                    ));
            }
            // We have been asked to act as a relay.
            protocol::RelayRemoteRequest::HopRequest(hop_request) => {
                self.queued_events
                    .push(RelayHandlerEvent::HopRequest(RelayHandlerHopRequest {
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
        println!("RelayHandler::inject_fully_negotiated_outbound");
        match protocol {
            // We have successfully negotiated a substream towards a destination.
            either::EitherOutput::First((substream_to_dest, dest_id)) => {
                self.queued_events
                    .push(RelayHandlerEvent::RelayRequestSuccess(dest_id, substream_to_dest));
            }
            // We have successfully asked the node to be a destination.
            either::EitherOutput::Second((to_dest_substream, hop_request)) => {
                self.accept_futures.push(hop_request.inner.fulfill(to_dest_substream));
            }
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            // Deny a relay request from the node we handle.
            RelayHandlerIn::DenyHopRequest(rq) => {
                let fut = rq.inner.deny();
                self.deny_futures.push(fut);
            }
            // Deny a destination request from the node we handle.
            RelayHandlerIn::DenyDestinationRequest(rq) => {
                let fut = rq.inner.deny();
                self.deny_futures.push(fut);
            }
            // Ask the node we handle to act as a relay.
            RelayHandlerIn::RelayRequest { target, addresses } => {
                self.relay_requests
                    .push((target.clone(), addresses.clone()));
            }
            // Ask the node we handle to act as a destination.
            RelayHandlerIn::DestinationRequest {
                source,
                source_addresses,
                substream,
            } => {
                self.dest_requests
                    .push((source, source_addresses, substream));
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        // TODO: Fix
        _: ProtocolsHandlerUpgrErr<either::EitherError<protocol::SendReadError, protocol::SendReadError>>,
    ) {
        // TODO:
        unimplemented!()
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes // TODO:
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
        Poll::Pending
        // // Request the remote to act as a relay.
        // if !self.relay_requests.is_empty() {
        //     let (peer_id, addrs) = self.relay_requests.remove(0);
        //     self.relay_requests.shrink_to_fit();
        //     return Poll::Ready(
        //         ProtocolsHandlerEvent::OutboundSubstreamRequest {
        //             info: (),
        //             protocol: SubstreamProtocol::new(upgrade::EitherUpgrade::A(protocol::RelayProxyRequest::new(
        //                 peer_id.clone(),
        //                 addrs,
        //                 peer_id,
        //             ))),
        //         },
        //     );
        // }

        // // Request the remote to act as destination.
        // if !self.dest_requests.is_empty() {
        //     let (source, source_addrs, substream) = self.dest_requests.remove(0);
        //     self.dest_requests.shrink_to_fit();
        //     return Ok(Poll::Ready(
        //         ProtocolsHandlerEvent::OutboundSubstreamRequest {
        //             info: (),
        //             protocol: SubstreamProtocol::new(upgrade::EitherUpgrade::B(protocol::RelayTargetOpen::new(
        //                 source,
        //                 source_addrs,
        //                 substream,
        //             ))),
        //         },
        //     ));
        // }

        // // Report the queued events.
        // if !self.queued_events.is_empty() {
        //     let event = self.queued_events.remove(0);
        //     return Ok(Poll::Ready(ProtocolsHandlerEvent::Custom(event)));
        // }

        // // Process the accept futures.
        // for n in (0..self.accept_futures.len()).rev() {
        //     let mut fut = self.accept_futures.swap_remove(n);
        //     match fut.poll() {
        //         // TODO: spawn in background task?
        //         Ok(Poll::Ready(copy_fut)) => self.copy_futures.push(copy_fut),
        //         Ok(Poll::Pending) => self.accept_futures.push(fut),
        //         Err(_err) => ()     // TODO: log this?
        //     }
        // }

        // // Process the futures.
        // self.deny_futures
        //     .retain(|f| f.poll().map(|a| a.is_not_ready()).unwrap_or(false));
        // self.copy_futures
        //     .retain(|f| f.poll().map(|a| a.is_not_ready()).unwrap_or(false));

        // Ok(Poll::Pending)
    }
}
