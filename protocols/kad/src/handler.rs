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

use crate::indexed_stream::IndexedStream;
use crate::protocol::{
    Codec, KadPeer, KadRequestMsg, KadResponseMsg, KademliaProtocolConfig, DEFAULT_MAX_PACKET_SIZE,
};
use crate::record::{self, Record};
use asynchronous_codec::{Responder, ReturnStream};
use either::Either;
use futures::prelude::*;
use futures::stream::SelectAll;
use instant::Instant;
use libp2p_core::{upgrade, ConnectedPoint};
use libp2p_identity::PeerId;
use libp2p_swarm::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    NegotiatedSubstream, SubstreamProtocol,
};
use log::trace;
use std::collections::VecDeque;
use std::{error, fmt, io, task::Context, task::Poll, time::Duration};

const MAX_NUM_SUBSTREAMS: usize = 32;

/// Protocol handler that manages substreams for the Kademlia protocol
/// on a single connection with a peer.
///
/// The handler will automatically open a Kademlia substream with the remote for each request we
/// make.
///
/// It also handles requests made by the remote.
pub struct KademliaHandler<TUserData> {
    /// Configuration for the Kademlia protocol.
    config: KademliaHandlerConfig,

    /// List of active outbound substreams with the state they are in.
    outbound_substreams_with_response: SelectAll<
        IndexedStream<
            TUserData,
            asynchronous_codec::SendRecv<
                NegotiatedSubstream,
                Codec<KadResponseMsg, KadRequestMsg>,
                ReturnStream,
            >,
        >,
    >,
    outbound_substreams_without_response: SelectAll<
        asynchronous_codec::Send<
            NegotiatedSubstream,
            Codec<KadResponseMsg, KadRequestMsg>,
            ReturnStream,
        >,
    >,

    idle_outbound_streams: VecDeque<NegotiatedSubstream>,

    /// Number of outbound streams being upgraded right now.
    num_requested_outbound_streams: usize,

    /// List of outbound substreams that are waiting to become active next.
    /// Contains the request we want to send, and the user data if we expect an answer.
    requested_streams:
        VecDeque<SubstreamProtocol<KademliaProtocolConfig, (KadRequestMsg, Option<TUserData>)>>,

    /// List of active inbound substreams with the state they are in.
    inbound_substreams: SelectAll<
        asynchronous_codec::RecvSend<
            NegotiatedSubstream,
            Codec<KadRequestMsg, KadResponseMsg>,
            ReturnStream,
        >,
    >,

    /// Until when to keep the connection alive.
    keep_alive: KeepAlive,

    /// The connected endpoint of the connection that the handler
    /// is associated with.
    endpoint: ConnectedPoint,

    /// The [`PeerId`] of the remote.
    remote_peer_id: PeerId,

    /// The current state of protocol confirmation.
    protocol_status: ProtocolStatus,

    events: VecDeque<KademliaHandlerEvent<TUserData>>,
}

/// The states of protocol confirmation that a connection
/// handler transitions through.
enum ProtocolStatus {
    /// It is as yet unknown whether the remote supports the
    /// configured protocol name.
    Unconfirmed,
    /// The configured protocol name has been confirmed by the remote
    /// but has not yet been reported to the `Kademlia` behaviour.
    Confirmed,
    /// The configured protocol has been confirmed by the remote
    /// and the confirmation reported to the `Kademlia` behaviour.
    Reported,
}

/// Configuration of a [`KademliaHandler`].
#[derive(Debug, Clone)]
pub struct KademliaHandlerConfig {
    /// Configuration of the wire protocol.
    pub protocol_config: KademliaProtocolConfig,

    /// If false, we deny incoming requests.
    pub allow_listening: bool,

    /// Time after which we close an idle connection.
    pub idle_timeout: Duration,
}

/// Event produced by the Kademlia handler.
#[derive(Debug)]
pub enum KademliaHandlerEvent<TUserData> {
    /// The configured protocol name has been confirmed by the peer through
    /// a successfully negotiated substream.
    ///
    /// This event is only emitted once by a handler upon the first
    /// successfully negotiated inbound or outbound substream and
    /// indicates that the connected peer participates in the Kademlia
    /// overlay network identified by the configured protocol name.
    ProtocolConfirmed { endpoint: ConnectedPoint },

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// The key for which to locate the closest nodes.
        key: Vec<u8>,
        responder: Responder<KadResponseMsg>, // TODO: Narrow down this type.
    },

    /// Response to an `KademliaHandlerIn::FindNodeReq`.
    FindNodeRes {
        /// Results of the request.
        closer_peers: Vec<KadPeer>,
        /// The user data passed to the `FindNodeReq`.
        user_data: TUserData,
    },

    /// Same as `FindNodeReq`, but should also return the entries of the local providers list for
    /// this key.
    GetProvidersReq {
        /// The key for which providers are requested.
        key: record::Key,
        responder: Responder<KadResponseMsg>, // TODO: Narrow down this type.
    },

    /// Response to an `KademliaHandlerIn::GetProvidersReq`.
    GetProvidersRes {
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// Known providers for this key.
        provider_peers: Vec<KadPeer>,
        /// The user data passed to the `GetProvidersReq`.
        user_data: TUserData,
    },

    /// An error happened when performing a query.
    QueryError {
        /// The error that happened.
        error: KademliaHandlerQueryErr,
        /// The user data passed to the query.
        user_data: TUserData,
    },

    /// The peer announced itself as a provider of a key.
    AddProvider {
        /// The key for which the peer is a provider of the associated value.
        key: record::Key,
        /// The peer that is the provider of the value for `key`.
        provider: KadPeer,
    },

    /// Request to get a value from the dht records
    GetRecord {
        /// Key for which we should look in the dht
        key: record::Key,
        responder: Responder<KadResponseMsg>, // TODO: Narrow down this type.
    },

    /// Response to a `KademliaHandlerIn::GetRecord`.
    GetRecordRes {
        /// The result is present if the key has been found
        record: Option<Record>,
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// The user data passed to the `GetValue`.
        user_data: TUserData,
    },

    /// Request to put a value in the dht records
    PutRecord {
        record: Record,
        responder: Responder<KadResponseMsg>, // TODO: Narrow down this type.
    },

    /// Response to a request to store a record.
    PutRecordRes {
        /// The key of the stored record.
        key: record::Key,
        /// The value of the stored record.
        value: Vec<u8>,
        /// The user data passed to the `PutValue`.
        user_data: TUserData,
    },
}

/// Error that can happen when requesting an RPC query.
#[derive(Debug)]
pub enum KademliaHandlerQueryErr {
    /// Error while trying to perform the query.
    Upgrade(ConnectionHandlerUpgrErr<io::Error>),
    /// Received an answer that doesn't correspond to the request.
    UnexpectedMessage,
    /// I/O error in the substream.
    Io(io::Error),
}

impl fmt::Display for KademliaHandlerQueryErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KademliaHandlerQueryErr::Upgrade(err) => {
                write!(f, "Error while performing Kademlia query: {err}")
            }
            KademliaHandlerQueryErr::UnexpectedMessage => {
                write!(
                    f,
                    "Remote answered our Kademlia RPC query with the wrong message type"
                )
            }
            KademliaHandlerQueryErr::Io(err) => {
                write!(f, "I/O error during a Kademlia RPC query: {err}")
            }
        }
    }
}

impl error::Error for KademliaHandlerQueryErr {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            KademliaHandlerQueryErr::Upgrade(err) => Some(err),
            KademliaHandlerQueryErr::UnexpectedMessage => None,
            KademliaHandlerQueryErr::Io(err) => Some(err),
        }
    }
}

impl From<ConnectionHandlerUpgrErr<quick_protobuf_codec::Error>> for KademliaHandlerQueryErr {
    fn from(err: ConnectionHandlerUpgrErr<quick_protobuf_codec::Error>) -> Self {
        KademliaHandlerQueryErr::Upgrade(
            err.map_upgrade_err(|e| e.map_err(|e| io::Error::new(io::ErrorKind::Other, e))),
        )
    }
}

impl From<quick_protobuf_codec::Error> for KademliaHandlerQueryErr {
    fn from(err: quick_protobuf_codec::Error) -> Self {
        KademliaHandlerQueryErr::Io(io::Error::new(io::ErrorKind::Other, err))
    }
}

/// Event to send to the handler.
#[derive(Debug)]
pub enum KademliaHandlerIn<TUserData> {
    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// Identifier of the node.
        key: Vec<u8>,
        /// Custom user data. Passed back in the out event when the results arrive.
        user_data: TUserData,
    },

    /// Same as `FindNodeReq`, but should also return the entries of the local providers list for
    /// this key.
    GetProvidersReq {
        /// Identifier being searched.
        key: record::Key,
        /// Custom user data. Passed back in the out event when the results arrive.
        user_data: TUserData,
    },

    /// Indicates that this provider is known for this key.
    ///
    /// The API of the handler doesn't expose any event that allows you to know whether this
    /// succeeded.
    AddProvider {
        /// Key for which we should add providers.
        key: record::Key,
        /// Known provider for this key.
        provider: KadPeer,
    },

    /// Request to retrieve a record from the DHT.
    GetRecord {
        /// The key of the record.
        key: record::Key,
        /// Custom data. Passed back in the out event when the results arrive.
        user_data: TUserData,
    },

    /// Put a value into the dht records.
    PutRecord {
        record: Record,
        /// Custom data. Passed back in the out event when the results arrive.
        user_data: TUserData,
    },
}

/// Unique identifier for a request. Must be passed back in order to answer a request from
/// the remote.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct KademliaRequestId {
    /// Unique identifier for an incoming connection.
    connec_unique_id: UniqueConnecId,
}

/// Unique identifier for a connection.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct UniqueConnecId(u64);

impl<TUserData> KademliaHandler<TUserData>
where
    TUserData: Clone + fmt::Debug + Send + 'static + Unpin,
{
    /// Create a [`KademliaHandler`] using the given configuration.
    pub fn new(
        config: KademliaHandlerConfig,
        endpoint: ConnectedPoint,
        remote_peer_id: PeerId,
    ) -> Self {
        let keep_alive = KeepAlive::Until(Instant::now() + config.idle_timeout);

        KademliaHandler {
            config,
            endpoint,
            remote_peer_id,
            inbound_substreams: Default::default(),
            outbound_substreams_with_response: Default::default(),
            outbound_substreams_without_response: Default::default(),
            idle_outbound_streams: Default::default(),
            num_requested_outbound_streams: 0,
            requested_streams: Default::default(),
            keep_alive,
            protocol_status: ProtocolStatus::Unconfirmed,
            events: Default::default(),
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol,
            info: (msg, user_data),
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        match user_data {
            Some(user_data) => {
                self.outbound_substreams_with_response
                    .push(IndexedStream::new(
                        user_data,
                        asynchronous_codec::SendRecv::new(
                            protocol,
                            Codec::new(DEFAULT_MAX_PACKET_SIZE),
                            msg,
                        ),
                    ));
            }
            None => {
                self.outbound_substreams_without_response
                    .push(asynchronous_codec::Send::new(
                        protocol,
                        Codec::new(DEFAULT_MAX_PACKET_SIZE),
                        msg,
                    ));
            }
        }

        self.num_requested_outbound_streams -= 1;
        if let ProtocolStatus::Unconfirmed = self.protocol_status {
            // Upon the first successfully negotiated substream, we know that the
            // remote is configured with the same protocol name and we want
            // the behaviour to add this peer to the routing table, if possible.
            self.protocol_status = ProtocolStatus::Confirmed;
        }
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound { protocol, .. }: FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
        // If `self.allow_listening` is false, then we produced a `DeniedUpgrade` and `protocol`
        // is a `Void`.
        let protocol = match protocol {
            future::Either::Left(p) => p,
            future::Either::Right(p) => void::unreachable(p),
        };

        if let ProtocolStatus::Unconfirmed = self.protocol_status {
            // Upon the first successfully negotiated substream, we know that the
            // remote is configured with the same protocol name and we want
            // the behaviour to add this peer to the routing table, if possible.
            self.protocol_status = ProtocolStatus::Confirmed;
        }

        if self.inbound_substreams.len() == MAX_NUM_SUBSTREAMS {
            log::warn!(
                "New inbound substream to {:?} exceeds inbound substream limit. \
                     No older substream waiting to be reused. Dropping new substream.",
                self.remote_peer_id,
            );
            return;
        }

        debug_assert!(self.config.allow_listening);

        self.inbound_substreams
            .push(asynchronous_codec::RecvSend::new(
                protocol,
                Codec::new(DEFAULT_MAX_PACKET_SIZE),
            ));
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError {
            info: (_, user_data),
            error,
            ..
        }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        // TODO: cache the fact that the remote doesn't support kademlia at all, so that we don't
        //       continue trying
        if let Some(user_data) = user_data {
            self.events.push_back(KademliaHandlerEvent::QueryError {
                error: error.into(),
                user_data,
            });
        }
        self.num_requested_outbound_streams -= 1;
    }
}

impl<TUserData> ConnectionHandler for KademliaHandler<TUserData>
where
    TUserData: Clone + fmt::Debug + Send + 'static + Unpin,
{
    type InEvent = KademliaHandlerIn<TUserData>;
    type OutEvent = KademliaHandlerEvent<TUserData>;
    type Error = io::Error; // TODO: better error type?
    type InboundProtocol = Either<KademliaProtocolConfig, upgrade::DeniedUpgrade>;
    type OutboundProtocol = KademliaProtocolConfig;
    // Message of the request to send to the remote, and user data if we expect an answer.
    type OutboundOpenInfo = (KadRequestMsg, Option<TUserData>);
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        if self.config.allow_listening {
            SubstreamProtocol::new(self.config.protocol_config.clone(), ())
                .map_upgrade(Either::Left)
        } else {
            SubstreamProtocol::new(Either::Right(upgrade::DeniedUpgrade), ())
        }
    }

    fn on_behaviour_event(&mut self, message: KademliaHandlerIn<TUserData>) {
        match message {
            KademliaHandlerIn::FindNodeReq { key, user_data } => {
                let msg = KadRequestMsg::FindNode { key };
                self.request_new_stream(Some(user_data), msg);
            }
            KademliaHandlerIn::GetProvidersReq { key, user_data } => {
                let msg = KadRequestMsg::GetProviders { key };
                self.request_new_stream(Some(user_data), msg);
            }
            KademliaHandlerIn::AddProvider { key, provider } => {
                let msg = KadRequestMsg::AddProvider { key, provider };
                self.request_new_stream(None, msg);
            }
            KademliaHandlerIn::GetRecord { key, user_data } => {
                let msg = KadRequestMsg::GetValue { key };
                self.request_new_stream(Some(user_data), msg);
            }
            KademliaHandlerIn::PutRecord { record, user_data } => {
                let msg = KadRequestMsg::PutValue { record };
                self.request_new_stream(Some(user_data), msg);
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::Custom(event));
        }

        if let ProtocolStatus::Confirmed = self.protocol_status {
            self.protocol_status = ProtocolStatus::Reported;
            return Poll::Ready(ConnectionHandlerEvent::Custom(
                KademliaHandlerEvent::ProtocolConfirmed {
                    endpoint: self.endpoint.clone(),
                },
            ));
        }

        while let Poll::Ready(Some((user_data, event))) =
            self.outbound_substreams_with_response.poll_next_unpin(cx)
        {
            match event {
                Ok((response, stream)) => {
                    self.idle_outbound_streams.push_back(stream);
                    return Poll::Ready(ConnectionHandlerEvent::Custom(process_kad_response(
                        response, user_data,
                    )));
                }
                Err(e) => {
                    trace!("Outbound substream error: {e}");
                }
            }
        }

        while let Poll::Ready(Some(event)) = self
            .outbound_substreams_without_response
            .poll_next_unpin(cx)
        {
            match event {
                Ok(stream) => {
                    self.idle_outbound_streams.push_back(stream);
                }
                Err(e) => {
                    trace!("Outbound substream error: {e}");
                }
            }
        }

        while let Poll::Ready(Some(event)) = self.inbound_substreams.poll_next_unpin(cx) {
            match event {
                Ok(asynchronous_codec::Event::Completed { stream }) => {
                    self.inbound_substreams
                        .push(asynchronous_codec::RecvSend::new(
                            stream,
                            Codec::new(DEFAULT_MAX_PACKET_SIZE),
                        ));
                }
                Ok(asynchronous_codec::Event::NewRequest { request, responder }) => {
                    return Poll::Ready(ConnectionHandlerEvent::Custom(match request {
                        KadRequestMsg::Ping => {
                            responder.respond(KadResponseMsg::Pong);
                            continue;
                        }
                        KadRequestMsg::FindNode { key } => {
                            KademliaHandlerEvent::FindNodeReq { key, responder }
                        }
                        KadRequestMsg::GetProviders { key } => {
                            KademliaHandlerEvent::GetProvidersReq { key, responder }
                        }
                        KadRequestMsg::AddProvider { key, provider } => {
                            drop(responder);
                            KademliaHandlerEvent::AddProvider { key, provider }
                        }
                        KadRequestMsg::GetValue { key } => {
                            KademliaHandlerEvent::GetRecord { key, responder }
                        }
                        KadRequestMsg::PutValue { record } => {
                            KademliaHandlerEvent::PutRecord { record, responder }
                        }
                    }));
                }
                Err(e) => {
                    trace!("Inbound substream error: {e}");
                }
            }
        }

        let num_in_progress_outbound_substreams =
            self.outbound_substreams_with_response.len() + self.num_requested_outbound_streams;
        if num_in_progress_outbound_substreams < MAX_NUM_SUBSTREAMS {
            if let Some(protocol) = self.requested_streams.pop_front() {
                self.num_requested_outbound_streams += 1;
                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol });
            }
        }

        if self.outbound_substreams_with_response.is_empty() && self.inbound_substreams.is_empty() {
            // We destroyed all substreams in this function.
            self.keep_alive = KeepAlive::Until(Instant::now() + self.config.idle_timeout);
        } else {
            self.keep_alive = KeepAlive::Yes;
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
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            ConnectionEvent::AddressChange(_) | ConnectionEvent::ListenUpgradeError(_) => {}
        }
    }
}

impl<TUserData> KademliaHandler<TUserData>
where
    TUserData: 'static + Clone + Send + Unpin + fmt::Debug,
{
    fn request_new_stream(&mut self, user_data: Option<TUserData>, msg: KadRequestMsg) {
        if let Some(idle_stream) = self.idle_outbound_streams.pop_front() {
            match user_data {
                Some(user_data) => {
                    self.outbound_substreams_with_response
                        .push(IndexedStream::new(
                            user_data,
                            asynchronous_codec::SendRecv::new(
                                idle_stream,
                                Codec::new(DEFAULT_MAX_PACKET_SIZE),
                                msg,
                            ),
                        ));
                }
                None => {
                    self.outbound_substreams_without_response
                        .push(asynchronous_codec::Send::new(
                            idle_stream,
                            Codec::new(DEFAULT_MAX_PACKET_SIZE),
                            msg,
                        ));
                }
            }
        } else {
            self.requested_streams.push_back(SubstreamProtocol::new(
                self.config.protocol_config.clone(),
                (msg, user_data),
            ));
        }
    }
}

impl Default for KademliaHandlerConfig {
    fn default() -> Self {
        KademliaHandlerConfig {
            protocol_config: Default::default(),
            allow_listening: true,
            idle_timeout: Duration::from_secs(10),
        }
    }
}

/// Process a Kademlia message that's supposed to be a response to one of our requests.
fn process_kad_response<TUserData>(
    event: KadResponseMsg,
    user_data: TUserData,
) -> KademliaHandlerEvent<TUserData> {
    // TODO: must check that the response corresponds to the request
    match event {
        KadResponseMsg::Pong => {
            // We never send out pings.
            KademliaHandlerEvent::QueryError {
                error: KademliaHandlerQueryErr::UnexpectedMessage,
                user_data,
            }
        }
        KadResponseMsg::FindNode { closer_peers } => KademliaHandlerEvent::FindNodeRes {
            closer_peers,
            user_data,
        },
        KadResponseMsg::GetProviders {
            closer_peers,
            provider_peers,
        } => KademliaHandlerEvent::GetProvidersRes {
            closer_peers,
            provider_peers,
            user_data,
        },
        KadResponseMsg::GetValue {
            record,
            closer_peers,
        } => KademliaHandlerEvent::GetRecordRes {
            record,
            closer_peers,
            user_data,
        },
        KadResponseMsg::PutValue { key, value, .. } => KademliaHandlerEvent::PutRecordRes {
            key,
            value,
            user_data,
        },
    }
}
