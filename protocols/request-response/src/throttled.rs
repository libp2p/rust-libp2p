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

// Limit the number of requests peers can send to each other.
//
// Each peer is assigned a budget for sending and a budget for receiving
// requests. Initially a peer assumes it has a send budget of 1. When its
// budget has been used up its remote peer will send a credit message which
// informs it how many more requests it can send before it needs to wait for
// the next credit message. Credit messages which error or time out are
// retried until they have reached the peer which is assumed once a
// corresponding ack or a new request has been received from the peer.
//
// The `Throttled` behaviour wraps an existing `RequestResponse` behaviour.
// and uses a codec implementation that sends ordinary requests and responses
// as well as a special credit message to which an ack message is expected
// as response. It does so by putting a small CBOR encoded header in front of
// each message the inner codec produces.

use crate::handler::{RequestProtocol, RequestResponseHandler, RequestResponseHandlerEvent};
use crate::codec::header::{self, Message};
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
pub struct Throttled<C>
where
    C: RequestResponseCodec + Send,
    C::Protocol: Sync
{
    /// A random id used for logging.
    id: u32,
    /// The wrapped behaviour.
    behaviour: RequestResponse<header::Codec<C>>,
    /// Information per peer.
    peer_info: HashMap<PeerId, PeerInfo>,
    /// The default limit applies to all peers unless overriden.
    default_limit: Limit,
    /// Permanent limit overrides per peer.
    limit_overrides: HashMap<PeerId, Limit>,
    /// Pending events to report in `Throttled::poll`.
    events: VecDeque<Event<C::Request, C::Response, header::Message<C::Response>>>,
    /// Current outbound credit grants in flight.
    credit_messages: HashMap<PeerId, Credit>,
    /// Counter of credit IDs.
    counter: u64
}

/// Credit information that is sent to remote peers.
#[derive(Clone, Copy, Debug)]
struct Credit {
    /// A credit ID. Used to deduplicate retransmitted credit messages.
    id: u64,
    /// The ID of the request carrying the credit grant.
    request: RequestId,
    /// The number of requests the remote is allowed to send.
    amount: u16
}

/// Max. number of inbound requests that can be received.
#[derive(Clone, Copy, Debug)]
struct Limit {
    /// The current receive limit.
    max_recv: NonZeroU16,
    /// The next receive limit which becomes active after
    /// the current limit has been reached.
    next_max: NonZeroU16
}

impl Limit {
    /// Create a new limit.
    fn new(max: NonZeroU16) -> Self {
        Limit { max_recv: max, next_max: max }
    }

    /// Set a new limit.
    ///
    /// The new limit becomes effective when all current inbound
    /// requests have been processed and replied to.
    fn set(&mut self, next: NonZeroU16) {
        self.next_max = next
    }

    /// Activate the new limit.
    fn switch(&mut self) -> u16 {
        self.max_recv = self.next_max;
        self.max_recv.get()
    }
}

/// Information about a peer.
#[derive(Clone, Debug)]
struct PeerInfo {
    /// Limit that applies to this peer.
    limit: Limit,
    /// Remaining number of outbound requests that can be sent.
    send_budget: u16,
    /// Remaining number of inbound requests that can be received.
    recv_budget: u16,
    /// The ID of the originating credit message.
    send_budget_id: Option<u64>,
    /// Current set of outbound requests.
    outbound: HashSet<RequestId>,
    /// Current set of inbound requests.
    inbound: HashSet<RequestId>
}

impl PeerInfo {
    fn new(limit: Limit) -> Self {
        PeerInfo {
            limit,
            send_budget: 1,
            recv_budget: 1,
            send_budget_id: None,
            outbound: HashSet::new(),
            inbound: HashSet::new()
        }
    }
}

impl<C> Throttled<C>
where
    C: RequestResponseCodec + Send + Clone,
    C::Protocol: Sync
{
    /// Wrap an existing `RequestResponse` behaviour and apply send/recv limits.
    pub fn new(behaviour: RequestResponse<header::Codec<C>>) -> Self {
        Throttled {
            id: rand::random(),
            behaviour,
            peer_info: HashMap::new(),
            default_limit: Limit::new(NonZeroU16::new(1).expect("1 > 0")),
            limit_overrides: HashMap::new(),
            events: VecDeque::new(),
            credit_messages: HashMap::new(),
            counter: 0
        }
    }

    /// Set the global default receive limit.
    pub fn set_receive_limit(&mut self, limit: NonZeroU16) {
        log::trace!("{:08x}: new default limit: {:?}", self.id, limit);
        self.default_limit = Limit::new(limit)
    }

    /// Override the receive limit of a single peer.
    pub fn override_receive_limit(&mut self, p: &PeerId, limit: NonZeroU16) {
        log::debug!("{:08x}: override limit for {}: {:?}", self.id, p, limit);
        if let Some(info) = self.peer_info.get_mut(p) {
            info.limit.set(limit)
        }
        self.limit_overrides.insert(p.clone(), Limit::new(limit));
    }

    /// Remove any limit overrides for the given peer.
    pub fn remove_override(&mut self, p: &PeerId) {
        log::trace!("{:08x}: removing limit override for {}", self.id, p);
        self.limit_overrides.remove(p);
    }

    /// Has the limit of outbound requests been reached for the given peer?
    pub fn can_send(&mut self, p: &PeerId) -> bool {
        self.peer_info.get(p).map(|i| i.send_budget > 0).unwrap_or(true)
    }

    /// Send a request to a peer.
    ///
    /// If the limit of outbound requests has been reached, the request is
    /// returned. Sending more outbound requests should only be attempted
    /// once [`Event::ResumeSending`] has been received from [`NetworkBehaviour::poll`].
    pub fn send_request(&mut self, p: &PeerId, req: C::Request) -> Result<RequestId, C::Request> {
        let info =
            if let Some(info) = self.peer_info.get_mut(p) {
                info
            } else {
                let limit = self.limit_overrides.get(p).copied().unwrap_or(self.default_limit);
                self.peer_info.entry(p.clone()).or_insert(PeerInfo::new(limit))
            };

        if info.send_budget == 0 {
            log::trace!("{:08x}: no more budget to send another request to {}", self.id, p);
            return Err(req)
        }

        info.send_budget -= 1;

        let rid = self.behaviour.send_request(p, Message::request(req));
        info.outbound.insert(rid);

        log::trace! { "{:08x}: sending request {} to {} (send budget = {})",
            self.id,
            rid,
            p,
            info.send_budget + 1
        };

        Ok(rid)
    }

    /// Answer an inbound request with a response.
    ///
    /// See [`RequestResponse::send_response`] for details.
    pub fn send_response(&mut self, ch: ResponseChannel<header::Message<C::Response>>, res: C::Response) {
        log::trace!("{:08x}: sending response {} to peer {}", self.id, ch.request_id(), &ch.peer);
        self.behaviour.send_response(ch, Message::response(res))
    }

    /// Add a known peer address.
    ///
    /// See [`RequestResponse::add_address`] for details.
    pub fn add_address(&mut self, p: &PeerId, a: Multiaddr) {
        self.behaviour.add_address(p, a)
    }

    /// Remove a previously added peer address.
    ///
    /// See [`RequestResponse::remove_address`] for details.
    pub fn remove_address(&mut self, p: &PeerId, a: &Multiaddr) {
        self.behaviour.remove_address(p, a)
    }

    /// Are we connected to the given peer?
    ///
    /// See [`RequestResponse::is_connected`] for details.
    pub fn is_connected(&self, p: &PeerId) -> bool {
        self.behaviour.is_connected(p)
    }

    /// Are we waiting for a response to the given request?
    ///
    /// See [`RequestResponse::is_pending_outbound`] for details.
    pub fn is_pending_outbound(&self, p: &RequestId) -> bool {
        self.behaviour.is_pending_outbound(p)
    }

    /// Create a new credit message ID.
    fn next_credit_id(&mut self) -> u64 {
        let n = self.counter;
        self.counter += 1;
        n
    }
}

/// A Wrapper around [`RequestResponseEvent`].
#[derive(Debug)]
pub enum Event<Req, Res, CRes = Res> {
    /// A regular request-response event.
    Event(RequestResponseEvent<Req, Res, CRes>),
    /// We received more inbound requests than allowed.
    TooManyInboundRequests(PeerId),
    /// When previously reaching the send limit of a peer,
    /// this event is eventually emitted when sending is
    /// allowed to resume.
    ResumeSending(PeerId)
}

impl<C> NetworkBehaviour for Throttled<C>
where
    C: RequestResponseCodec + Send + Clone + 'static,
    C::Protocol: Sync
{
    type ProtocolsHandler = RequestResponseHandler<header::Codec<C>>;
    type OutEvent = Event<C::Request, C::Response, header::Message<C::Response>>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.behaviour.new_handler()
    }

    fn addresses_of_peer(&mut self, p: &PeerId) -> Vec<Multiaddr> {
        self.behaviour.addresses_of_peer(p)
    }

    fn inject_connection_established(&mut self, p: &PeerId, id: &ConnectionId, end: &ConnectedPoint) {
        self.behaviour.inject_connection_established(p, id, end)
    }

    fn inject_connection_closed(&mut self, peer: &PeerId, id: &ConnectionId, end: &ConnectedPoint) {
        if let Some(credit) = self.credit_messages.get_mut(peer) {
            log::debug! { "{:08x}: resending credit grant {} to {} after connection closed",
                self.id,
                credit.id,
                peer
            };
            let msg = Message::credit(credit.amount, credit.id);
            credit.request = self.behaviour.send_request(peer, msg)
        }
        self.behaviour.inject_connection_closed(peer, id, end)
    }

    fn inject_connected(&mut self, p: &PeerId) {
        log::trace!("{:08x}: connected to {}", self.id, p);
        self.behaviour.inject_connected(p);
        // The limit may have been added by `Throttled::send_request` already.
        if !self.peer_info.contains_key(p) {
            let limit = self.limit_overrides.get(p).copied().unwrap_or(self.default_limit);
            self.peer_info.insert(p.clone(), PeerInfo::new(limit));
        }
    }

    fn inject_disconnected(&mut self, p: &PeerId) {
        log::trace!("{:08x}: disconnected from {}", self.id, p);
        self.peer_info.remove(p);
        self.behaviour.inject_disconnected(p)
    }

    fn inject_dial_failure(&mut self, p: &PeerId) {
        self.behaviour.inject_dial_failure(p)
    }

    fn inject_event(&mut self, p: PeerId, i: ConnectionId, e: RequestResponseHandlerEvent<header::Codec<C>>) {
        if let RequestResponseHandlerEvent::ResponseSent(r) = &e {
            if let Some(info) = self.peer_info.get_mut(&p) {
                if info.inbound.remove(r) {
                    log::trace!("{:08x}: response {} sent", self.id, r);
                    if info.recv_budget == 0 { // need to send more credit to the remote peer
                        let crd = info.limit.switch();
                        info.recv_budget = info.limit.max_recv.get();
                        let cid = self.next_credit_id();
                        let rid = self.behaviour.send_request(&p, Message::credit(crd, cid));
                        log::trace!("{:08x}: sending {} as credit {} to {}", self.id, crd, cid, p);
                        let credit = Credit { id: cid, request: rid, amount: crd };
                        self.credit_messages.insert(p.clone(), credit);
                    }
                }
            }
        }
        self.behaviour.inject_event(p, i, e)
    }

    fn poll(&mut self, cx: &mut Context<'_>, params: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<RequestProtocol<header::Codec<C>>, Self::OutEvent>>
    {
        loop {
            if let Some(ev) = self.events.pop_front() {
                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(ev))
            } else if self.events.capacity() > super::EMPTY_QUEUE_SHRINK_THRESHOLD {
                self.events.shrink_to_fit()
            }

            let event = match ready!(self.behaviour.poll(cx, params)) {
                | NetworkBehaviourAction::GenerateEvent(RequestResponseEvent::Message { peer, message }) => {
                    let message = match message {
                        | RequestResponseMessage::Response { request_id, response } =>
                            match &response.header().typ {
                                | Some(header::Type::Ack) => {
                                    if let Some(id) = self.credit_messages.get(&peer).map(|c| c.id) {
                                        if Some(id) == response.header().ident {
                                            log::trace!("{:08x}: received ack {} from {}", self.id, id, peer);
                                            self.credit_messages.remove(&peer);
                                        }
                                    }
                                    continue
                                }
                                | Some(header::Type::Response) =>
                                    if let Some(info) = self.peer_info.get_mut(&peer) {
                                        if info.outbound.remove(&request_id) {
                                            log::trace!("{:08x}: received response {} from {}", self.id, request_id, peer);
                                            if let Some(rs) = response.into_parts().1 {
                                                RequestResponseMessage::Response { request_id, response: rs }
                                            } else {
                                                log::error! { "{:08x}: missing data for response {} from peer {}",
                                                    self.id,
                                                    request_id,
                                                    peer
                                                }
                                                continue
                                            }
                                        } else {
                                            log::warn!("{:08x}: unexpected response {} from {}", self.id, request_id, peer);
                                            continue
                                        }
                                    } else {
                                        log::warn!("{:08x}: response {} from unknown peer {}", self.id, request_id, peer);
                                        continue
                                    }
                                | ty => {
                                    log::trace! {
                                        "{:08x}: unknown message type: {:?} from {}; expected response or credit",
                                        self.id,
                                        ty,
                                        peer
                                    };
                                    continue
                                }
                            }
                        | RequestResponseMessage::Request { request_id, request, channel } =>
                            match &request.header().typ {
                                | Some(header::Type::Credit) => {
                                    let id = if let Some(n) = request.header().ident {
                                        n
                                    } else {
                                        log::warn! { "{:08x}: missing credit id in message from {}",
                                            self.id,
                                            peer
                                        }
                                        continue
                                    };
                                    let c = request.header().credit.unwrap_or(0);
                                    log::trace! { "{:08x}: received {} additional credit {} from {}",
                                        self.id,
                                        c,
                                        id,
                                        peer
                                    };
                                    if let Some(info) = self.peer_info.get_mut(&peer) {
                                        if info.send_budget_id >= Some(id) {
                                            log::trace!("{:08x}: ignoring duplicate credit {} from {}", self.id, id, peer);
                                            continue
                                        }
                                        if info.send_budget == 0 && c > 0 {
                                            log::trace!("{:08x}: sending to peer {} can resume", self.id, peer);
                                            self.events.push_back(Event::ResumeSending(peer.clone()))
                                        }
                                        info.send_budget += c;
                                        info.send_budget_id = Some(id)
                                    }
                                    self.behaviour.send_response(channel, Message::ack(id));
                                    continue
                                }
                                | Some(header::Type::Request) => {
                                    if let Some(info) = self.peer_info.get_mut(&peer) {
                                        log::trace! { "{:08x}: received request {} (recv. budget = {})",
                                            self.id,
                                            request_id,
                                            info.recv_budget
                                        };
                                        if info.recv_budget == 0 {
                                            log::error!("{:08x}: peer {} exceeds its budget", self.id, peer);
                                            self.events.push_back(Event::TooManyInboundRequests(peer.clone()));
                                            continue
                                        }
                                        info.inbound.insert(request_id);
                                        info.recv_budget -= 1;
                                        // We consider a request as proof that our credit grant has
                                        // reached the peer. Usually, an ACK has already been
                                        // received.
                                        self.credit_messages.remove(&peer);
                                    }
                                    if let Some(rq) = request.into_parts().1 {
                                        RequestResponseMessage::Request { request_id, request: rq, channel }
                                    } else {
                                        log::error! { "{:08x}: missing data for request {} from peer {}",
                                            self.id,
                                            request_id,
                                            peer
                                        }
                                        continue
                                    }
                                }
                                | ty => {
                                    log::trace! {
                                        "{:08x}: unknown message type: {:?} from {}; expected request or ack",
                                        self.id,
                                        ty,
                                        peer
                                    };
                                    continue
                                }
                            }
                    };
                    let event = RequestResponseEvent::Message { peer, message };
                    NetworkBehaviourAction::GenerateEvent(Event::Event(event))
                }
                | NetworkBehaviourAction::GenerateEvent(RequestResponseEvent::OutboundFailure {
                    peer,
                    request_id,
                    error
                }) => {
                    if let Some(credit) = self.credit_messages.get_mut(&peer) {
                        if credit.request == request_id {
                            log::debug! { "{:08x}: failed to send {} as credit {} to {}; retrying...",
                                self.id,
                                credit.amount,
                                credit.id,
                                peer
                            };
                            let msg = Message::credit(credit.amount, credit.id);
                            credit.request = self.behaviour.send_request(&peer, msg)
                        }
                    }
                    let event = RequestResponseEvent::OutboundFailure { peer, request_id, error };
                    NetworkBehaviourAction::GenerateEvent(Event::Event(event))
                }
                | NetworkBehaviourAction::GenerateEvent(RequestResponseEvent::InboundFailure {
                    peer,
                    request_id,
                    error
                }) => {
                    let event = RequestResponseEvent::InboundFailure { peer, request_id, error };
                    NetworkBehaviourAction::GenerateEvent(Event::Event(event))
                }
                | NetworkBehaviourAction::DialAddress { address } =>
                    NetworkBehaviourAction::DialAddress { address },
                | NetworkBehaviourAction::DialPeer { peer_id, condition } =>
                    NetworkBehaviourAction::DialPeer { peer_id, condition },
                | NetworkBehaviourAction::NotifyHandler { peer_id, handler, event } =>
                    NetworkBehaviourAction::NotifyHandler { peer_id, handler, event },
                | NetworkBehaviourAction::ReportObservedAddr { address } =>
                    NetworkBehaviourAction::ReportObservedAddr { address }
            };

            return Poll::Ready(event)
        }
    }
}
