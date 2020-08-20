// Copyright 2020 Parity Technologies (UK) Ltd.
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

use crate::handler::{RequestProtocol, RequestResponseHandler, RequestResponseHandlerEvent};
use futures::ready;
use libp2p_core::{ConnectedPoint, connection::ConnectionId, Multiaddr, PeerId};
use libp2p_swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use std::{collections::{HashMap, HashSet, VecDeque}, task::{Context, Poll}};
use std::num::NonZeroU16;
use super::{
    RequestId,
    RequestResponse,
    RequestResponseCodec,
    RequestResponseEvent,
    RequestResponseMessage,
    ResponseChannel
};

/// A wrapper around [`RequestResponse`] which adds request limits per peer.
///
/// Each peer is assigned a default limit of concurrent requests and
/// responses, which can be overriden per peer.
///
/// It is not possible to send more requests than configured and receiving
/// more is reported as an error event. Since `libp2p-request-response` is
/// not its own protocol, there is no way to communicate limits to peers,
/// hence nodes must have pre-established knowledge about each other's limits.
pub struct Throttled<C: RequestResponseCodec> {
    /// A random id used for logging.
    id: u32,
    /// The wrapped behaviour.
    behaviour: RequestResponse<C>,
    /// Information per peer.
    limits: HashMap<PeerId, Info>,
    /// The default limits applied to all peers unless overriden.
    default: Info,
    /// Pending events to report in `Throttled::poll`.
    events: VecDeque<Event<C::Request, C::Response>>,
}

#[derive(Clone, Debug)]
struct Info {
    /// The remaining number of outbound requests that can be send.
    send_budget: u16,
    /// The remaining number of inbound requests that can be received.
    recv_budget: u16,
    /// The original limit which applies to inbound and outbound requests.
    maximum: NonZeroU16,
    /// Current outbound requests.
    outbound: HashSet<RequestId>,
    /// Current inbound requests.
    inbound: HashSet<RequestId>
}

impl Default for Info {
    fn default() -> Self {
        let maximum = NonZeroU16::new(1).expect("1 > 0");
        Info {
            send_budget: maximum.get(),
            recv_budget: maximum.get(),
            maximum,
            outbound: HashSet::new(),
            inbound: HashSet::new()
        }
    }
}

/// A Wrapper around [`RequestResponseEvent`].
#[derive(Debug)]
pub enum Event<Req, Res> {
    /// A regular request-response event.
    Event(RequestResponseEvent<Req, Res>),
    /// We received more inbound requests than allowed.
    TooManyInboundRequests(PeerId),
    /// When previously reaching the send limit of a peer,
    /// this event is eventually emitted when sending is
    /// allowed to resume.
    ResumeSending(PeerId)
}

impl<C: RequestResponseCodec + Clone> Throttled<C> {
    /// Wrap an existing `RequestResponse` behaviour and apply send/recv limits.
    pub fn new(behaviour: RequestResponse<C>) -> Self {
        Throttled {
            id: rand::random(),
            behaviour,
            limits: HashMap::new(),
            default: Info::default(),
            events: VecDeque::new()
        }
    }

    /// Get the current default limit applied to all peers.
    pub fn default_limit(&self) -> u16 {
        self.default.maximum.get()
    }

    /// Override the global default limit.
    pub fn set_default_limit(&mut self, limit: NonZeroU16) {
        log::trace!("{:08x}: new default limit: {:?}", self.id, limit);
        self.default = Info {
            send_budget: limit.get(),
            recv_budget: limit.get(),
            maximum: limit,
            outbound: HashSet::new(),
            inbound: HashSet::new()
        }
    }

    /// Has the limit of outbound requests been reached for the given peer?
    pub fn can_send(&mut self, id: &PeerId) -> bool {
        self.limits.get(id).map(|l| l.send_budget > 0).unwrap_or(true)
    }

    /// Send a request to a peer.
    ///
    /// If the limit of outbound requests has been reached, the request is
    /// returned. Sending more outbound requests should only be attempted
    /// once [`Event::ResumeSending`] has been received from [`NetworkBehaviour::poll`].
    pub fn send_request(&mut self, id: &PeerId, req: C::Request) -> Result<RequestId, C::Request> {
        log::trace!("{:08x}: sending request to {}", self.id, id);

        let mut limit =
            if let Some(limit) = self.limits.get_mut(id) {
                limit
            } else {
                self.limits.entry(id.clone()).or_insert(self.default.clone())
            };

        if limit.send_budget == 0 {
            log::trace!("{:08x}: no budget to send request to {}", self.id, id);
            return Err(req)
        }

        limit.send_budget -= 1;

        let rid = self.behaviour.send_request(id, req);
        limit.outbound.insert(rid);
        Ok(rid)
    }

    /// Answer an inbound request with a response.
    ///
    /// See [`RequestResponse::send_response`] for details.
    pub fn send_response(&mut self, ch: ResponseChannel<C::Response>, rs: C::Response) {
        if let Some(limit) = self.limits.get_mut(&ch.peer) {
            if limit.inbound.remove(&ch.request_id()) {
                limit.recv_budget += 1;
                debug_assert!(limit.recv_budget <= limit.maximum.get());
            }
        }
        self.behaviour.send_response(ch, rs)
    }

    /// Add a known peer address.
    ///
    /// See [`RequestResponse::add_address`] for details.
    pub fn add_address(&mut self, id: &PeerId, ma: Multiaddr) {
        self.behaviour.add_address(id, ma)
    }

    /// Remove a previously added peer address.
    ///
    /// See [`RequestResponse::remove_address`] for details.
    pub fn remove_address(&mut self, id: &PeerId, ma: &Multiaddr) {
        self.behaviour.remove_address(id, ma)
    }

    /// Are we connected to the given peer?
    ///
    /// See [`RequestResponse::is_connected`] for details.
    pub fn is_connected(&self, id: &PeerId) -> bool {
        self.behaviour.is_connected(id)
    }

    /// Are we waiting for a response to the given request?
    ///
    /// See [`RequestResponse::is_pending_outbound`] for details.
    pub fn is_pending_outbound(&self, id: &RequestId) -> bool {
        self.behaviour.is_pending_outbound(id)
    }

    /// Are we still processing and inbound request?
    ///
    /// See [`RequestResponse::is_pending_inbound`] for details.
    pub fn is_pending_inbound(&self, id: &RequestId) -> bool {
        self.behaviour.is_pending_inbound(id)
    }

    /// Update the limits when a request resolved.
    fn on_outbound_request_end(&mut self, p: &PeerId, r: &RequestId) {
        if let Some(limit) = self.limits.get_mut(p) {
            if limit.outbound.remove(r) {
                if limit.send_budget == 0 {
                    log::trace!("{:08x}: sending to peer {} can resume", self.id, p);
                    self.events.push_back(Event::ResumeSending(p.clone()))
                }
                limit.send_budget += 1;
                debug_assert!(limit.send_budget <= limit.maximum.get())
            }
        }
    }
}

impl<C> NetworkBehaviour for Throttled<C>
where
    C: RequestResponseCodec + Send + Clone + 'static
{
    type ProtocolsHandler = RequestResponseHandler<C>;
    type OutEvent = Event<C::Request, C::Response>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.behaviour.new_handler()
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.behaviour.addresses_of_peer(peer)
    }

    fn inject_connection_established(&mut self, p: &PeerId, id: &ConnectionId, end: &ConnectedPoint) {
        self.behaviour.inject_connection_established(p, id, end)
    }

    fn inject_connection_closed(&mut self, p: &PeerId, id: &ConnectionId, end: &ConnectedPoint) {
        self.behaviour.inject_connection_closed(p, id, end)
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        log::trace!("{:08x}: connected to {}", self.id, peer);
        self.behaviour.inject_connected(peer);
        // The limit may have been added by `Throttled::send_request` already.
        if !self.limits.contains_key(peer) {
            self.limits.insert(peer.clone(), self.default.clone());
        }
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        log::trace!("{:08x}: disconnected from {}", self.id, peer);
        self.limits.remove(peer);
        self.behaviour.inject_disconnected(peer);
    }

    fn inject_dial_failure(&mut self, peer: &PeerId) {
        self.behaviour.inject_dial_failure(peer)
    }

    fn inject_event(&mut self, p: PeerId, i: ConnectionId, e: RequestResponseHandlerEvent<C>) {
        self.behaviour.inject_event(p, i, e)
    }

    fn poll(&mut self, cx: &mut Context<'_>, p: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<RequestProtocol<C>, Self::OutEvent>>
    {
        if let Some(ev) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(ev))
        } else if self.events.capacity() > super::EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.events.shrink_to_fit()
        }

        loop {
            let event = ready!(self.behaviour.poll(cx, p));

            match event {
                NetworkBehaviourAction::GenerateEvent(RequestResponseEvent::Message { ref peer, ref message }) =>
                    match message {
                        RequestResponseMessage::Response { ref request_id, .. } =>
                            self.on_outbound_request_end(peer, request_id),
                        RequestResponseMessage::Request { ref request_id, .. } =>
                            if let Some(limit) = self.limits.get_mut(peer) {
                                if limit.recv_budget == 0 {
                                    log::error!("{:08x}: peer {} exceeds its budget", self.id, peer);
                                    self.events.push_back(Event::TooManyInboundRequests(peer.clone()));
                                    continue
                                }
                                limit.inbound.insert(*request_id);
                                limit.recv_budget -= 1
                            }
                    }

                NetworkBehaviourAction::GenerateEvent(RequestResponseEvent::OutboundFailure {
                    ref peer,
                    ref request_id,
                    ..
                }) => self.on_outbound_request_end(peer, request_id),

                NetworkBehaviourAction::GenerateEvent(RequestResponseEvent::InboundFailure {
                    ref peer,
                    ref request_id,
                    ..
                }) => {
                    if let Some(limit) = self.limits.get_mut(peer) {
                        if limit.inbound.remove(request_id) {
                            limit.recv_budget += 1;
                            debug_assert!(limit.recv_budget <= limit.maximum.get())
                        }
                    }
                }

                _ => ()
            };

            return Poll::Ready(event.map_out(Event::Event))
        }
    }
}
