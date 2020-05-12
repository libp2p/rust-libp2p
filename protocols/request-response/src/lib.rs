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

//! Generic request/response protocols.
//!
//! ## General Usage
//!
//! [`RequestResponse`] is a `NetworkBehaviour` that implements a generic
//! request/response protocol whereby each request is sent over a new
//! substream on a connection. `RequestResponse` is generic over the
//! actual messages being sent, which are defined in terms of a
//! [`RequestResponseCodec`]. Creating a request/response protocol thus amounts
//! to providing an implementation of this trait that can be
//! given to [`RequestResponse::new`]. Further configuration is available
//! via the [`RequestResponseConfig`].
//!
//! Requests are sent using [`RequestResponse::send_request`] and the
//! responses received as [`RequestResponseMessage::Response`] via
//! [`RequestResponseEvent::Message`].
//!
//! Responses are sent using [`RequestResponse::send_response`] upon
//! receiving a [`RequestResponseMessage::Request`] via
//! [`RequestResponseEvent::Message`].
//!
//! ## One-Way Protocols
//!
//! The implementation supports one-way protocols that do not
//! have responses. In these cases the [`RequestResponseCodec::Response`] can
//! be defined as `()` and [`RequestResponseCodec::read_response`] as well as
//! [`RequestResponseCodec::write_response`] given the obvious implementations.
//! Note that `RequestResponseMessage::Response` will still be emitted,
//! immediately after the request has been sent, since `RequestResponseCodec::read_response`
//! will not actually read anything from the given I/O stream.
//! [`RequestResponse::send_response`] need not be called for one-way protocols.

use bytes::Bytes;
use futures::{
    future::BoxFuture,
    prelude::*,
    stream::FuturesUnordered
};
use libp2p_core::{
    ConnectedPoint,
    Multiaddr,
    PeerId,
    ProtocolName,
    connection::ConnectionId,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
};
use libp2p_swarm::{
    DialPeerCondition,
    NegotiatedSubstream,
    NetworkBehaviour,
    NetworkBehaviourAction,
    NotifyHandler,
    OneShotEvent,
    OneShotHandler,
    OneShotHandlerConfig,
    OneShotOutboundInfo,
    PollParameters,
    SubstreamProtocol
};
use smallvec::SmallVec;
use std::{
    collections::{VecDeque, HashMap},
    io,
    iter,
    time::Duration,
    task::{Context, Poll}
};

/// An inbound request or response.
#[derive(Debug)]
pub enum RequestResponseMessage<TRequest, TResponse> {
    /// A request message.
    Request {
        /// The request message.
        request: TRequest,
        /// The sender of the request who is awaiting a response.
        ///
        /// See [`RequestResponse::send_response`].
        channel: ResponseChannel,
    },
    /// A response message.
	Response {
        /// The ID of the request that produced this response.
        ///
        /// See [`RequestResponse::send_request`].
        request_id: RequestId,
        /// The response message.
        response: TResponse
    },
}

/// The events emitted by a [`RequestResponse`] protocol.
#[derive(Debug)]
pub enum RequestResponseEvent<TRequest, TResponse> {
    /// An incoming message (request or response).
    Message {
        /// The peer who sent the message.
        peer: PeerId,
        /// The incoming message.
        message: RequestResponseMessage<TRequest, TResponse>
    },
    /// An outbound request failed.
    RequestFailure {
        /// The peer to whom the request was sent.
        peer: PeerId,
        /// The (local) ID of the failed request.
        request_id: RequestId,
        /// The error that occurred.
        error: RequestFailure
    },
    /// Sending of a response to an incoming request failed.
    ///
    /// See [`RequestResponse::send_response`].
    ResponseFailure {
        /// The error that occurred.
        error: io::Error
    },
}

/// Possible request failures.
#[derive(Debug)]
pub enum RequestFailure {
    /// The request could not be sent because a dialing attempt failed.
    DialFailure,
    /// The request timed out before receiving a response.
    Timeout,
    /// The connection closed before a response was received.
    ///
    /// It is not known whether the request may have been
    /// received (and processed) by the remote peer.
    ConnectionClosed,
}

/// A channel for sending a response to an inbound request.
///
/// See [`RequestResponse::send_response`].
#[derive(Debug)]
pub struct ResponseChannel(NegotiatedSubstream);

/// The ID of an outgoing request.
///
/// See [`RequestResponse::send_request`].
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

/// The configuration for an `RequestResponse`.
#[derive(Debug, Clone)]
pub struct RequestResponseConfig {
    protocol: Bytes,
    request_timeout: Duration,
    connection_keep_alive: Duration,
}

impl RequestResponseConfig {
    /// Creates a new `RequestResponseConfig` for the given protocol.
    pub fn new(protocol: impl ProtocolName) -> Self {
        Self {
            protocol: Bytes::copy_from_slice(protocol.protocol_name()),
            connection_keep_alive: Duration::from_secs(10),
            request_timeout: Duration::from_secs(10),
        }
    }

    /// Sets the keep-alive timeout of idle connections.
    pub fn set_connection_keep_alive(&mut self, v: Duration) -> &mut Self {
        self.connection_keep_alive = v;
        self
    }

    /// Sets the request timeout for outbound requests.
    pub fn set_request_timeout(&mut self, v: Duration) -> &mut Self {
        self.request_timeout = v;
        self
    }
}

/// A request/response protocol for some message codec.
pub struct RequestResponse<TCodec>
where
    TCodec: RequestResponseCodec,
{
    /// The next (local) request ID.
    next_request_id: RequestId,
    /// The protocol configuration.
    config: RequestResponseConfig,
    /// The protocol codec for reading and writing requests and responses.
    codec: TCodec,
    /// Pending futures for outgoing responses.
    responding: FuturesUnordered<BoxFuture<'static, Result<(), io::Error>>>,
    /// Pending events to return from `poll`.
    pending_events: VecDeque<
        NetworkBehaviourAction<
            RequestProtocol<TCodec>,
            RequestResponseEvent<TCodec::Request, TCodec::Response>>>,
    /// The currently connected peers and their known, reachable addresses, if any.
    connected: HashMap<PeerId, SmallVec<[Connection; 2]>>,
    /// Externally managed addresses via `add_address` and `remove_address`.
    addresses: HashMap<PeerId, SmallVec<[Multiaddr; 6]>>,
    /// Requests that have not yet been sent and are waiting for a connection
    /// to be established.
    pending_requests: HashMap<PeerId, SmallVec<[RequestProtocol<TCodec>; 10]>>,
    /// Responses that have not yet been received.
    pending_responses: HashMap<RequestId, (PeerId, ConnectionId)>,
}

impl<TCodec> RequestResponse<TCodec>
where
    TCodec: RequestResponseCodec + Clone,
{
    /// Creates a new `RequestResponse` protocol for the given codec and configuration.
    pub fn new(cfg: RequestResponseConfig, codec: TCodec) -> Self {
        RequestResponse {
            next_request_id: RequestId(1),
            config: cfg,
            codec,
            responding: FuturesUnordered::new(),
            pending_events: VecDeque::new(),
            connected: HashMap::new(),
            pending_requests: HashMap::new(),
            pending_responses: HashMap::new(),
            addresses: HashMap::new(),
        }
    }

    /// Initiates sending a request.
    ///
    /// If the targeted peer is currently not connected, a dialing
    /// attempt is initiated and the request is sent as soon as a
    /// connection is established.
    ///
    /// > **Note**: In order for such a dialing attempt to succeed,
    /// > the `RequestResonse` protocol must either be embedded
    /// > in another `NetworkBehaviour` that provides peer and
    /// > address discovery, or known addresses of peers must be
    /// > managed via [`RequestResponse::add_address`] and
    /// > [`RequestResponse::remove_address`].
    pub fn send_request(&mut self, peer: &PeerId, request: TCodec::Request) -> RequestId {
        let request_id = self.next_request_id();
        let request = RequestProtocol {
            request_id,
            codec: self.codec.clone(),
            protocol: self.config.protocol.clone(),
            request,
        };

        if let Some(request) = self.try_send_request(peer, request) {
            self.pending_events.push_back(NetworkBehaviourAction::DialPeer {
                peer_id: peer.clone(),
                condition: DialPeerCondition::Disconnected,
            });
            self.pending_requests.entry(peer.clone()).or_default().push(request);
        }

        request_id
    }

    /// Initiates sending a response to an inbound request.
    ///
    /// The provided `ResponseChannel` is obtained from a
    /// [`RequestResponseMessage::Request`].
    pub fn send_response(&mut self, mut channel: ResponseChannel, response: TCodec::Response)
    where
        TCodec: RequestResponseCodec + Send + Sync + 'static,
        TCodec::Response: Send + 'static,
    {
        let mut codec = self.codec.clone();
        self.responding.push(async move {
            codec.write_response(&mut channel.0, response).await
        }.boxed());
    }

    /// Adds a known address for a peer that can be used for
    /// dialing attempts by the `Swarm`, i.e. is returned
    /// by [`NetworkBehaviour::addresses_of_peer`].
    ///
    /// Addresses added in this way are only removed by `remove_address`.
    pub fn add_address(&mut self, peer: &PeerId, address: Multiaddr) {
        self.addresses.entry(peer.clone()).or_default().push(address);
    }

    /// Removes an address of a peer previously added via `add_address`.
    pub fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) {
        let mut last = false;
        if let Some(addresses) = self.addresses.get_mut(peer) {
            addresses.retain(|a| a != address);
            last = addresses.is_empty();
        }
        if last {
            self.addresses.remove(peer);
        }
    }

    /// Checks whether a peer is currently connected.
    pub fn is_connected(&self, peer: &PeerId) -> bool {
        self.connected.contains_key(peer)
    }

    /// Returns the next request ID.
    fn next_request_id(&mut self) -> RequestId {
        let request_id = self.next_request_id;
        self.next_request_id.0 += 1;
        request_id
    }

    /// Tries to send a request by queueing an appropriate event to be
    /// emitted to the `Swarm`. If the peer is not currently connected,
    /// the given request is return unchanged.
    fn try_send_request(&mut self, peer: &PeerId, request: RequestProtocol<TCodec>)
        -> Option<RequestProtocol<TCodec>>
    {
        if let Some(connections) = self.connected.get(peer) {
            let ix = (request.request_id.0 as usize) % connections.len();
            let conn = connections[ix].id;
            self.pending_responses.insert(request.request_id, (peer.clone(), conn));
            self.pending_events.push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: peer.clone(),
                handler: NotifyHandler::One(conn),
                event: request
            });
            None
        } else {
            Some(request)
        }
    }
}

impl<TCodec> NetworkBehaviour for RequestResponse<TCodec>
where
    TCodec: RequestResponseCodec + Send + Sync + Clone + 'static,
    TCodec::Response: Send,
    TCodec::Request: Send,
{
    type ProtocolsHandler = OneShotHandler<
        ResponseProtocol<TCodec>,
        RequestProtocol<TCodec>,
        RequestResponseMessage<TCodec::Request, TCodec::Response>,
        RequestId,
    >;

    type OutEvent = RequestResponseEvent<TCodec::Request, TCodec::Response>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        let inbound = SubstreamProtocol::new(ResponseProtocol {
            protocol: self.config.protocol.clone(),
            codec: self.codec.clone(),
        }).with_timeout(self.config.request_timeout);
        OneShotHandler::new(inbound, OneShotHandlerConfig {
            keep_alive_timeout: self.config.connection_keep_alive,
            outbound_substream_timeout: self.config.request_timeout,
        })
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        let mut addresses = Vec::new();
        if let Some(connections) = self.connected.get(peer) {
            addresses.extend(connections.iter().filter_map(|c| c.address.clone()))
        }
        if let Some(more) = self.addresses.get(peer) {
            addresses.extend(more.into_iter().cloned());
        }
        addresses
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        if let Some(pending) = self.pending_requests.remove(peer) {
            for request in pending {
                let request = self.try_send_request(peer, request);
                assert!(request.is_none());
            }
        }
    }

    fn inject_connection_established(&mut self, peer: &PeerId, conn: &ConnectionId, endpoint: &ConnectedPoint) {
        let address = match endpoint {
            ConnectedPoint::Dialer { address } => Some(address.clone()),
            ConnectedPoint::Listener { .. } => None
        };
        let connections = self.connected.entry(peer.clone()).or_default();
        connections.push(Connection { id: *conn, address })
    }

    fn inject_connection_closed(&mut self, peer: &PeerId, conn: &ConnectionId, _: &ConnectedPoint) {
        if let Some(connections) = self.connected.get_mut(peer) {
            if let Some(pos) = connections.iter().position(|c| &c.id == conn) {
                connections.remove(pos);
            }
        }

        // Any pending responses of requests sent over this connection
        // must be considered failed.
        let failed = self.pending_responses.iter()
            .filter_map(|(r, (p, c))|
                if conn == c {
                    Some((p.clone(), *r))
                } else {
                    None
                })
            .collect::<Vec<_>>();

        for (peer, request_id) in failed {
            self.pending_responses.remove(&request_id);
            self.pending_events.push_back(NetworkBehaviourAction::GenerateEvent(
                RequestResponseEvent::RequestFailure {
                    peer,
                    request_id,
                    error: RequestFailure::ConnectionClosed
                }
            ));
        }
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        self.connected.remove(peer);
    }

    fn inject_dial_failure(&mut self, peer: &PeerId) {
        // If there are pending outgoing requests when a dial failure occurs,
        // it is implied that we are not connected to the peer, since pending
        // outgoing requests are drained when a connection is established and
        // only created when a peer is not connected when a request is made.
        // Thus these requests must be considered failed, even if there is
        // another, concurrent dialing attempt ongoing.
        if let Some(pending) = self.pending_requests.remove(peer) {
            for request in pending {
                self.pending_events.push_back(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::RequestFailure {
                        peer: peer.clone(),
                        request_id: request.request_id,
                        error: RequestFailure::DialFailure
                    }
                ));
            }
        }
    }

    fn inject_event(
        &mut self,
        peer: PeerId,
        _conn: ConnectionId,
        event: OneShotEvent<RequestResponseMessage<TCodec::Request, TCodec::Response>, RequestId>
    ) {
        match event {
            OneShotEvent::Success(message) => {
                if let RequestResponseMessage::Response { request_id, .. } = &message {
                    self.pending_responses.remove(request_id);
                }
                self.pending_events.push_back(
                    NetworkBehaviourAction::GenerateEvent(
                        RequestResponseEvent::Message { peer, message }));
            }
            OneShotEvent::Timeout(request_id) => {
                if let Some((peer, _conn)) = self.pending_responses.remove(&request_id) {
                    self.pending_events.push_back(
                        NetworkBehaviourAction::GenerateEvent(
                            RequestResponseEvent::RequestFailure {
                                peer,
                                request_id,
                                error: RequestFailure::Timeout,
                            }));
                }
            }
        }
    }

    fn poll(&mut self, cx: &mut Context, _: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<
            RequestProtocol<TCodec>,
            RequestResponseEvent<TCodec::Request, TCodec::Response>
        >>
    {
        if let Some(ev) = self.pending_events.pop_front() {
            return Poll::Ready(ev);
        }

        while let Poll::Ready(Some(result)) = self.responding.poll_next_unpin(cx) {
            if let Err(error) = result {
                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::ResponseFailure { error }
                ));
            }
        }

        Poll::Pending
    }
}

/// Response substream upgrade protocol.
#[derive(Debug, Clone)]
pub struct ResponseProtocol<TCodec> {
    protocol: Bytes,
    codec: TCodec,
}

impl<TCodec> UpgradeInfo for ResponseProtocol<TCodec> {
    type Info = Bytes;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.protocol.clone())
    }
}

impl<TCodec> InboundUpgrade<NegotiatedSubstream> for ResponseProtocol<TCodec>
where
    TCodec: RequestResponseCodec + Send + Sync + 'static,
{
    type Output = RequestResponseMessage<TCodec::Request, TCodec::Response>;
    type Error = io::Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(mut self, mut io: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        async move {
            let request = self.codec.read_request(&mut io).await?;
            Ok(RequestResponseMessage::Request { request, channel: ResponseChannel(io) })
        }.boxed()
    }
}

/// Request substream upgrade protocol.
#[derive(Debug, Clone)]
pub struct RequestProtocol<TCodec>
where
    TCodec: RequestResponseCodec
{
    codec: TCodec,
    protocol: Bytes,
    request: TCodec::Request,
    request_id: RequestId,
}

impl<TCodec> OneShotOutboundInfo<RequestId> for RequestProtocol<TCodec>
where
    TCodec: RequestResponseCodec
{
    fn info(&self) -> RequestId {
        self.request_id
    }
}

impl<TCodec> UpgradeInfo for RequestProtocol<TCodec>
where
    TCodec: RequestResponseCodec
{
    type Info = Bytes;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.protocol.clone())
    }
}

impl<TCodec> OutboundUpgrade<NegotiatedSubstream> for RequestProtocol<TCodec>
where
    TCodec: RequestResponseCodec + Send + Sync + 'static,
    TCodec::Request: Send + 'static,
{
    type Output = RequestResponseMessage<TCodec::Request, TCodec::Response>;
    type Error = io::Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(mut self, mut io: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        async move {
            self.codec.write_request(&mut io, self.request).await?;
            let response = self.codec.read_response(&mut io).await?;
            Ok(RequestResponseMessage::Response { request_id: self.request_id, response })
        }.boxed()
    }
}

/// An `RequestResponseCodec` defines the request and response types
/// for a [`RequestResponse`] protocol and how they are encoded / decoded
/// to / from an I/O stream.
pub trait RequestResponseCodec {
    type Request;
    type Response;

    fn read_request<'a, T>(&mut self, io: &'a mut T)
        -> BoxFuture<'a, Result<Self::Request, io::Error>>
    where
        T: AsyncRead + Unpin + Send;

    fn read_response<'a, T>(&mut self, io: &'a mut T)
        -> BoxFuture<'a, Result<Self::Response, io::Error>>
    where
        T: AsyncRead + Unpin + Send;

    fn write_request<'a, T>(&mut self, io: &'a mut T, req: Self::Request)
        -> BoxFuture<'a, Result<(), io::Error>>
    where
        T: AsyncWrite + Unpin + Send;

    fn write_response<'a, T>(&mut self, io: &'a mut T, res: Self::Response)
        -> BoxFuture<'a, Result<(), io::Error>>
    where
        T: AsyncWrite + Unpin + Send;
}

/// Internal information tracked for an established connection.
struct Connection {
    id: ConnectionId,
    address: Option<Multiaddr>,
}
