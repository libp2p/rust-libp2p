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
use libp2p_core::{upgrade, ConnectedPoint, Multiaddr, PeerId};
use libp2p_swarm::{
    IntoProtocolsHandler, KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use log::warn;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::io;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

pub struct RelayHandlerConfig {
    pub connection_idle_timeout: Duration,
}

pub struct RelayHandlerProto {
    pub config: RelayHandlerConfig,
}

impl IntoProtocolsHandler for RelayHandlerProto {
    type Handler = RelayHandler;

    fn into_handler(self, _: &PeerId, endpoint: &ConnectedPoint) -> Self::Handler {
        RelayHandler::new(self.config, endpoint.get_remote_address().clone())
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        protocol::RelayListen::new()
    }
}

/// Protocol handler that handles the relay protocol.
///
/// There are four possible situations in play here:
///
/// - The handler emits `RelayHandlerEvent::IncomingRelayReq` if the node we handle asks us to
///   act as a relay. You must send a `RelayHandlerIn::OutgoingDstReq` to another
///   handler, or send back a `DenyIncomingRelayReq`.
///
/// - The handler emits `RelayHandlerEvent::IncomingDstReq` if the node we handle asks
///   us to act as a destination. You must either call `accept` on the produced object, or send back
///   a `DenyDstReq`.
///
/// - Send a `RelayHandlerIn::OutgoingRelayReq` if the node we handle must act as a relay to a
///   destination. The handler will either send back a `RelayReqSuccess` containing the stream
///   to the destination, or a `OutgoingRelayReqDenied`.
///
/// - Send a `RelayHandlerIn::OutgoingDstReq` if the node we handle must act as a
///   destination. The handler will automatically notify the source whether the request was accepted
///   or denied.
///
pub struct RelayHandler {
    config: RelayHandlerConfig,
    /// Specifies whether the connection handled by this Handler is used to listen for incoming
    /// relayed connections.
    used_for_listening: bool,
    remote_address: Multiaddr,
    /// Futures that send back negative responses.
    deny_futures: FuturesUnordered<BoxFuture<'static, Result<(), std::io::Error>>>,

    incoming_dst_req_pending_approval:
        HashMap<RequestId, protocol::IncomingDstReq<NegotiatedSubstream>>,

    /// Futures that send back an accept response to a relay.
    accept_dst_futures: FuturesUnordered<
        BoxFuture<
            'static,
            Result<
                (PeerId, protocol::Connection<NegotiatedSubstream>, oneshot::Receiver<()>),
                protocol::IncomingDstReqError,
            >,
        >,
    >,

    /// Futures that copy from a source to a destination.
    copy_futures:
        FuturesUnordered<BoxFuture<'static, Result<(), protocol::IncomingRelayReqError>>>,

    /// Requests asking the remote to become a relay.
    outgoing_relay_reqs: Vec<OutgoingRelayReq>,

    /// Requests asking the remote to become a destination.
    outgoing_dst_reqs: SmallVec<
        [(
            PeerId,
            RequestId,
            Multiaddr,
            protocol::IncomingRelayReq<NegotiatedSubstream>,
        ); 4],
    >,

    /// Queue of events to return when polled.
    queued_events: Vec<RelayHandlerEvent>,

    /// Tracks substreams lend out to other [`Relayandler`]s or as
    /// [`Connection`](protocol::Connection) to the
    /// [`RelayTransportWrapper`](crate::RelayTransportWrapper).
    ///
    /// For each substream to the peer of this handler, there is a future in here that resolves once
    /// the given substream is dropped.
    ///
    /// Once all substreams are dropped and this handler has no other work, [`KeepAlive::Until`] can
    /// be set eventually allowing the connection to be closed.
    alive_lend_out_substreams: FuturesUnordered<oneshot::Receiver<()>>,

    keep_alive: KeepAlive,
}

struct OutgoingRelayReq {
    dst_peer_id: PeerId,
    request_id: RequestId,
    /// Addresses of the destination.
    dst_addr: Multiaddr,
}

/// Event produced by the relay handler.
pub enum RelayHandlerEvent {
    /// The remote wants us to relay communications to a third party. You must either send back a
    /// `DenyIncomingRelayReq`, or send a `OutgoingDstReq` to a different handler containing
    /// this object.
    IncomingRelayReq {
        request_id: RequestId,
        src_addr: Multiaddr,
        req: protocol::IncomingRelayReq<NegotiatedSubstream>,
    },

    /// The remote is a relay and is relaying a connection to us. In other words, we are used as
    /// destination. The behaviour can accept or deny the request via [`AcceptDstReq`]
    /// or [`DenyDstReq`].
    IncomingDstReq(PeerId, RequestId),

    /// A `RelayReq` that has previously been sent has been accepted by the remote. Contains
    /// a substream that communicates with the requested destination.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the destination. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    OutgoingRelayReqSuccess(PeerId, RequestId, protocol::Connection<NegotiatedSubstream>),

    /// The local node has accepted an incoming destination request. Contains a substream that
    /// communicates with the source.
    ///
    /// > **Note**: There is no proof that we are actually communicating with the destination. An
    /// >           encryption handshake has to be performed on top of this substream in order to
    /// >           avoid MITM attacks.
    IncomingRelayReqSuccess {
        stream: protocol::Connection<NegotiatedSubstream>,
        src: PeerId,
    },

    /// A `RelayReq` that has previously been sent by the local node has failed.
    OutgoingRelayReqError(PeerId, RequestId),
}

/// Event that can be sent to the relay handler.
pub enum RelayHandlerIn {
    /// Tell the handler whether it is handling a connection used to listen for incoming relayed
    /// connections.
    UsedForListening(bool),
    /// Denies a relay request sent by the node we talk to acting as a source.
    DenyIncomingRelayReq(protocol::IncomingRelayReq<NegotiatedSubstream>),

    /// Denies a destination request sent by the node we talk to.
    DenyDstReq(PeerId, RequestId),

    /// Accepts a destination request sent by the node we talk to.
    AcceptDstReq(PeerId, RequestId),

    /// Opens a new substream to the remote and asks it to relay communications to a third party.
    OutgoingRelayReq {
        dst_peer_id: PeerId,
        request_id: RequestId,
        /// Addresses known for this peer to transmit to the remote.
        dst_addr: Multiaddr,
    },

    /// Asks the node to be used as a destination for a relayed connection.
    ///
    /// The positive or negative response will be written to `substream`.
    OutgoingDstReq {
        /// Peer id of the node whose communications are being relayed.
        src: PeerId,
        request_id: RequestId,
        /// Address of the node whose communications are being relayed.
        src_addr: Multiaddr,
        /// Substream to the source.
        substream: protocol::IncomingRelayReq<NegotiatedSubstream>,
    },
}

impl RelayHandler {
    /// Builds a new `RelayHandler`.
    pub fn new(config: RelayHandlerConfig, remote_address: Multiaddr) -> Self {
        RelayHandler {
            config,
            used_for_listening: false,
            remote_address,
            deny_futures: Default::default(),
            incoming_dst_req_pending_approval: Default::default(),
            accept_dst_futures: Default::default(),
            copy_futures: Default::default(),
            outgoing_relay_reqs: Default::default(),
            outgoing_dst_reqs: Default::default(),
            queued_events: Default::default(),
            alive_lend_out_substreams: Default::default(),
            keep_alive: KeepAlive::Yes,
        }
    }
}

impl ProtocolsHandler for RelayHandler {
    type InEvent = RelayHandlerIn;
    type OutEvent = RelayHandlerEvent;
    type Error = io::Error;
    type InboundProtocol = protocol::RelayListen;
    type OutboundProtocol = upgrade::EitherUpgrade<
        protocol::OutgoingRelayReq,
        protocol::OutgoingDstReq<protocol::IncomingRelayReq<NegotiatedSubstream>>,
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
            protocol::RelayRemoteReq::RelayReq((incoming_relay_request, notifyee)) => {
                self.alive_lend_out_substreams.push(notifyee);
                self.queued_events
                    .push(RelayHandlerEvent::IncomingRelayReq {
                        request_id,
                        src_addr: self.remote_address.clone(),
                        req: incoming_relay_request,
                    });
            }
            // We have been asked to become a destination.
            protocol::RelayRemoteReq::DstReq(dest_request) => {
                let src = dest_request.src_id().clone();
                self.incoming_dst_req_pending_approval
                    .insert(request_id, dest_request);
                self.queued_events
                    .push(RelayHandlerEvent::IncomingDstReq(
                        src, request_id,
                    ));
            }
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as upgrade::OutboundUpgrade<NegotiatedSubstream>>::Output,
        (dst_peer_id, request_id): Self::OutboundOpenInfo,
    ) {
        match protocol {
            // We have successfully negotiated a substream towards a relay.
            EitherOutput::First((substream_to_dest, notifyee)) => {
                self.alive_lend_out_substreams.push(notifyee);
                self.queued_events
                    .push(RelayHandlerEvent::OutgoingRelayReqSuccess(
                        dst_peer_id,
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
            RelayHandlerIn::UsedForListening(s) => self.used_for_listening = s,
            // Deny a relay request from the node we handle.
            RelayHandlerIn::DenyIncomingRelayReq(rq) => {
                let fut = rq.deny();
                self.deny_futures.push(fut);
            }
            RelayHandlerIn::AcceptDstReq(_src, request_id) => {
                let rq = self
                    .incoming_dst_req_pending_approval
                    .remove(&request_id)
                    .unwrap();
                let fut = rq.accept();
                self.accept_dst_futures.push(fut);
            }
            // Deny a destination request from the node we handle.
            RelayHandlerIn::DenyDstReq(_src, request_id) => {
                let rq = self
                    .incoming_dst_req_pending_approval
                    .remove(&request_id)
                    .unwrap();
                let fut = rq.deny();
                self.deny_futures.push(fut);
            }
            // Ask the node we handle to act as a relay.
            RelayHandlerIn::OutgoingRelayReq {
                dst_peer_id,
                request_id,
                dst_addr,
            } => {
                self.outgoing_relay_reqs.push(OutgoingRelayReq {
                    dst_peer_id,
                    request_id,
                    dst_addr,
                });
            }
            // Ask the node we handle to act as a destination.
            RelayHandlerIn::OutgoingDstReq {
                src,
                request_id,
                src_addr,
                substream,
            } => {
                self.outgoing_dst_reqs.push((
                    src,
                    request_id,
                    src_addr,
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
                protocol::OutgoingRelayReqError,
                protocol::OutgoingDstReqError,
            >,
        >,
    ) {
        match error {
            ProtocolsHandlerUpgrErr::Upgrade(upgrade::UpgradeError::Apply(EitherError::A(_))) => {
                self.queued_events
                    .push(RelayHandlerEvent::OutgoingRelayReqError(
                        peer_id, request_id,
                    ));
            }
            // TODO: When the outbound destination request fails, send a status update back to the
            // source.
            e => panic!("{:?}", e),
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
        if !self.outgoing_relay_reqs.is_empty() {
            let OutgoingRelayReq {
                dst_peer_id,
                request_id,
                dst_addr,
            } = self.outgoing_relay_reqs.remove(0);
            self.outgoing_relay_reqs.shrink_to_fit();
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    upgrade::EitherUpgrade::A(protocol::OutgoingRelayReq::new(
                        dst_peer_id,
                        dst_addr,
                    )),
                    (dst_peer_id, request_id),
                ),
            });
        }

        // Request the remote to act as destination.
        if !self.outgoing_dst_reqs.is_empty() {
            let (src, request_id, src_addr, substream) =
                self.outgoing_dst_reqs.remove(0);
            self.outgoing_dst_reqs.shrink_to_fit();
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    upgrade::EitherUpgrade::B(protocol::OutgoingDstReq::new(
                        src.clone(),
                        src_addr,
                        substream,
                    )),
                    (src, request_id),
                ),
            });
        }

        match self.accept_dst_futures.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok((src, substream, notifyee)))) => {
                self.alive_lend_out_substreams.push(notifyee);
                let event = RelayHandlerEvent::IncomingRelayReqSuccess {
                    stream: substream,
                    src,
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

        if self.used_for_listening
            || !self.deny_futures.is_empty()
            || !self.accept_dst_futures.is_empty()
            || !self.copy_futures.is_empty()
            || !self.alive_lend_out_substreams.is_empty()
        {
            // Protocol handler is busy.
            self.keep_alive = KeepAlive::Yes;
        } else {
            // Protocol handler is idle.
            if matches!(self.keep_alive, KeepAlive::Yes) {
                self.keep_alive = KeepAlive::Until(Instant::now() + self.config.connection_idle_timeout);
            }
        }

        Poll::Pending
    }
}
