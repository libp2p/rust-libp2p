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

use crate::behaviour::Mode;
use crate::protocol::{
    KadInStreamSink, KadOutStreamSink, KadPeer, KadRequestMsg, KadResponseMsg,
    KademliaProtocolConfig,
};
use crate::record_priv::{self, Record};
use crate::QueryId;
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
    ConnectionHandler, ConnectionHandlerEvent, ConnectionId, KeepAlive, Stream, StreamUpgradeError,
    SubstreamProtocol, SupportedProtocols,
};
use log::trace;
use std::collections::VecDeque;
use std::task::Waker;
use std::{
    error, fmt, io, marker::PhantomData, pin::Pin, task::Context, task::Poll, time::Duration,
};

const MAX_NUM_SUBSTREAMS: usize = 32;

/// Protocol handler that manages substreams for the Kademlia protocol
/// on a single connection with a peer.
///
/// The handler will automatically open a Kademlia substream with the remote for each request we
/// make.
///
/// It also handles requests made by the remote.
pub struct KademliaHandler {
    /// Configuration of the wire protocol.
    protocol_config: KademliaProtocolConfig,

    /// In client mode, we don't accept inbound substreams.
    mode: Mode,

    /// Time after which we close an idle connection.
    idle_timeout: Duration,

    /// Next unique ID of a connection.
    next_connec_unique_id: UniqueConnecId,

    /// List of active outbound substreams with the state they are in.
    outbound_substreams: SelectAll<OutboundSubstreamState>,

    /// Number of outbound streams being upgraded right now.
    num_requested_outbound_streams: usize,

    /// List of outbound substreams that are waiting to become active next.
    /// Contains the request we want to send, and the user data if we expect an answer.
    pending_messages: VecDeque<(KadRequestMsg, Option<QueryId>)>,

    /// List of active inbound substreams with the state they are in.
    inbound_substreams: SelectAll<InboundSubstreamState>,

    /// Until when to keep the connection alive.
    keep_alive: KeepAlive,

    /// The connected endpoint of the connection that the handler
    /// is associated with.
    endpoint: ConnectedPoint,

    /// The [`PeerId`] of the remote.
    remote_peer_id: PeerId,

    /// The current state of protocol confirmation.
    protocol_status: ProtocolStatus,

    remote_supported_protocols: SupportedProtocols,

    /// The ID of this connection.
    connection_id: ConnectionId,
}

/// The states of protocol confirmation that a connection
/// handler transitions through.
#[derive(Copy, Clone)]
enum ProtocolStatus {
    /// It is as yet unknown whether the remote supports the
    /// configured protocol name.
    Unknown,
    /// The configured protocol name has been confirmed by the remote
    /// but has not yet been reported to the `Kademlia` behaviour.
    Confirmed,
    /// The configured protocol name(s) are not or no longer supported by the remote.
    NotSupported,
    /// The configured protocol has been confirmed by the remote
    /// and the confirmation reported to the `Kademlia` behaviour.
    Reported,
}

/// State of an active outbound substream.
enum OutboundSubstreamState {
    /// Waiting to send a message to the remote.
    PendingSend(KadOutStreamSink<Stream>, KadRequestMsg, Option<QueryId>),
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(KadOutStreamSink<Stream>, Option<QueryId>),
    /// Waiting for an answer back from the remote.
    // TODO: add timeout
    WaitingAnswer(KadOutStreamSink<Stream>, QueryId),
    /// An error happened on the substream and we should report the error to the user.
    ReportError(KademliaHandlerQueryErr, QueryId),
    /// The substream is being closed.
    Closing(KadOutStreamSink<Stream>),
    /// The substream is complete and will not perform any more work.
    Done,
    Poisoned,
}

/// State of an active inbound substream.
enum InboundSubstreamState {
    /// Waiting for a request from the remote.
    WaitingMessage {
        /// Whether it is the first message to be awaited on this stream.
        first: bool,
        connection_id: UniqueConnecId,
        substream: KadInStreamSink<Stream>,
    },
    /// Waiting for the behaviour to send a [`KademliaHandlerIn`] event containing the response.
    WaitingBehaviour(UniqueConnecId, KadInStreamSink<Stream>, Option<Waker>),
    /// Waiting to send an answer back to the remote.
    PendingSend(UniqueConnecId, KadInStreamSink<Stream>, KadResponseMsg),
    /// Waiting to flush an answer back to the remote.
    PendingFlush(UniqueConnecId, KadInStreamSink<Stream>),
    /// The substream is being closed.
    Closing(KadInStreamSink<Stream>),
    /// The substream was cancelled in favor of a new one.
    Cancelled,

    Poisoned {
        phantom: PhantomData<QueryId>,
    },
}

impl InboundSubstreamState {
    fn try_answer_with(
        &mut self,
        id: KademliaRequestId,
        msg: KadResponseMsg,
    ) -> Result<(), KadResponseMsg> {
        match std::mem::replace(
            self,
            InboundSubstreamState::Poisoned {
                phantom: PhantomData,
            },
        ) {
            InboundSubstreamState::WaitingBehaviour(conn_id, substream, mut waker)
                if conn_id == id.connec_unique_id =>
            {
                *self = InboundSubstreamState::PendingSend(conn_id, substream, msg);

                if let Some(waker) = waker.take() {
                    waker.wake();
                }

                Ok(())
            }
            other => {
                *self = other;

                Err(msg)
            }
        }
    }

    fn close(&mut self) {
        match std::mem::replace(
            self,
            InboundSubstreamState::Poisoned {
                phantom: PhantomData,
            },
        ) {
            InboundSubstreamState::WaitingMessage { substream, .. }
            | InboundSubstreamState::WaitingBehaviour(_, substream, _)
            | InboundSubstreamState::PendingSend(_, substream, _)
            | InboundSubstreamState::PendingFlush(_, substream)
            | InboundSubstreamState::Closing(substream) => {
                *self = InboundSubstreamState::Closing(substream);
            }
            InboundSubstreamState::Cancelled => {
                *self = InboundSubstreamState::Cancelled;
            }
            InboundSubstreamState::Poisoned { .. } => unreachable!(),
        }
    }
}

/// Event produced by the Kademlia handler.
#[derive(Debug)]
pub enum KademliaHandlerEvent {
    /// The configured protocol name has been confirmed by the peer through
    /// a successfully negotiated substream or by learning the supported protocols of the remote.
    ProtocolConfirmed { endpoint: ConnectedPoint },
    /// The configured protocol name(s) are not or no longer supported by the peer on the provided
    /// connection and it should be removed from the routing table.
    ProtocolNotSupported { endpoint: ConnectedPoint },

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// The key for which to locate the closest nodes.
        key: Vec<u8>,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: KademliaRequestId,
    },

    /// Response to an `KademliaHandlerIn::FindNodeReq`.
    FindNodeRes {
        /// Results of the request.
        closer_peers: Vec<KadPeer>,
        /// The user data passed to the `FindNodeReq`.
        query_id: QueryId,
    },

    /// Same as `FindNodeReq`, but should also return the entries of the local providers list for
    /// this key.
    GetProvidersReq {
        /// The key for which providers are requested.
        key: record_priv::Key,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: KademliaRequestId,
    },

    /// Response to an `KademliaHandlerIn::GetProvidersReq`.
    GetProvidersRes {
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// Known providers for this key.
        provider_peers: Vec<KadPeer>,
        /// The user data passed to the `GetProvidersReq`.
        query_id: QueryId,
    },

    /// An error happened when performing a query.
    QueryError {
        /// The error that happened.
        error: KademliaHandlerQueryErr,
        /// The user data passed to the query.
        query_id: QueryId,
    },

    /// The peer announced itself as a provider of a key.
    AddProvider {
        /// The key for which the peer is a provider of the associated value.
        key: record_priv::Key,
        /// The peer that is the provider of the value for `key`.
        provider: KadPeer,
    },

    /// Request to get a value from the dht records
    GetRecord {
        /// Key for which we should look in the dht
        key: record_priv::Key,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: KademliaRequestId,
    },

    /// Response to a `KademliaHandlerIn::GetRecord`.
    GetRecordRes {
        /// The result is present if the key has been found
        record: Option<Record>,
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// The user data passed to the `GetValue`.
        query_id: QueryId,
    },

    /// Request to put a value in the dht records
    PutRecord {
        record: Record,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: KademliaRequestId,
    },

    /// Response to a request to store a record.
    PutRecordRes {
        /// The key of the stored record.
        key: record_priv::Key,
        /// The value of the stored record.
        value: Vec<u8>,
        /// The user data passed to the `PutValue`.
        query_id: QueryId,
    },
}

/// Error that can happen when requesting an RPC query.
#[derive(Debug)]
pub enum KademliaHandlerQueryErr {
    /// Error while trying to perform the query.
    Upgrade(StreamUpgradeError<io::Error>),
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

impl From<StreamUpgradeError<io::Error>> for KademliaHandlerQueryErr {
    fn from(err: StreamUpgradeError<io::Error>) -> Self {
        KademliaHandlerQueryErr::Upgrade(err)
    }
}

/// Event to send to the handler.
#[derive(Debug)]
pub enum KademliaHandlerIn {
    /// Resets the (sub)stream associated with the given request ID,
    /// thus signaling an error to the remote.
    ///
    /// Explicitly resetting the (sub)stream associated with a request
    /// can be used as an alternative to letting requests simply time
    /// out on the remote peer, thus potentially avoiding some delay
    /// for the query on the remote.
    Reset(KademliaRequestId),

    /// Change the connection to the specified mode.
    ReconfigureMode { new_mode: Mode },

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// Identifier of the node.
        key: Vec<u8>,
        /// Custom user data. Passed back in the out event when the results arrive.
        query_id: QueryId,
    },

    /// Response to a `FindNodeReq`.
    FindNodeRes {
        /// Results of the request.
        closer_peers: Vec<KadPeer>,
        /// Identifier of the request that was made by the remote.
        ///
        /// It is a logic error to use an id of the handler of a different node.
        request_id: KademliaRequestId,
    },

    /// Same as `FindNodeReq`, but should also return the entries of the local providers list for
    /// this key.
    GetProvidersReq {
        /// Identifier being searched.
        key: record_priv::Key,
        /// Custom user data. Passed back in the out event when the results arrive.
        query_id: QueryId,
    },

    /// Response to a `GetProvidersReq`.
    GetProvidersRes {
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// Known providers for this key.
        provider_peers: Vec<KadPeer>,
        /// Identifier of the request that was made by the remote.
        ///
        /// It is a logic error to use an id of the handler of a different node.
        request_id: KademliaRequestId,
    },

    /// Indicates that this provider is known for this key.
    ///
    /// The API of the handler doesn't expose any event that allows you to know whether this
    /// succeeded.
    AddProvider {
        /// Key for which we should add providers.
        key: record_priv::Key,
        /// Known provider for this key.
        provider: KadPeer,
    },

    /// Request to retrieve a record from the DHT.
    GetRecord {
        /// The key of the record.
        key: record_priv::Key,
        /// Custom data. Passed back in the out event when the results arrive.
        query_id: QueryId,
    },

    /// Response to a `GetRecord` request.
    GetRecordRes {
        /// The value that might have been found in our storage.
        record: Option<Record>,
        /// Nodes that are closer to the key we were searching for.
        closer_peers: Vec<KadPeer>,
        /// Identifier of the request that was made by the remote.
        request_id: KademliaRequestId,
    },

    /// Put a value into the dht records.
    PutRecord {
        record: Record,
        /// Custom data. Passed back in the out event when the results arrive.
        query_id: QueryId,
    },

    /// Response to a `PutRecord`.
    PutRecordRes {
        /// Key of the value that was put.
        key: record_priv::Key,
        /// Value that was put.
        value: Vec<u8>,
        /// Identifier of the request that was made by the remote.
        request_id: KademliaRequestId,
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

impl KademliaHandler {
    pub fn new(
        protocol_config: KademliaProtocolConfig,
        idle_timeout: Duration,
        endpoint: ConnectedPoint,
        remote_peer_id: PeerId,
        mode: Mode,
        connection_id: ConnectionId,
    ) -> Self {
        match &endpoint {
            ConnectedPoint::Dialer { .. } => {
                log::debug!(
                    "Operating in {mode}-mode on new outbound connection to {remote_peer_id}"
                );
            }
            ConnectedPoint::Listener { .. } => {
                log::debug!(
                    "Operating in {mode}-mode on new inbound connection to {remote_peer_id}"
                );
            }
        }

        let keep_alive = KeepAlive::Until(Instant::now() + idle_timeout);

        KademliaHandler {
            protocol_config,
            mode,
            idle_timeout,
            endpoint,
            remote_peer_id,
            next_connec_unique_id: UniqueConnecId(0),
            inbound_substreams: Default::default(),
            outbound_substreams: Default::default(),
            num_requested_outbound_streams: 0,
            pending_messages: Default::default(),
            keep_alive,
            protocol_status: ProtocolStatus::Unknown,
            remote_supported_protocols: Default::default(),
            connection_id,
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound { protocol, info: () }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        if let Some((msg, query_id)) = self.pending_messages.pop_front() {
            self.outbound_substreams
                .push(OutboundSubstreamState::PendingSend(protocol, msg, query_id));
        } else {
            debug_assert!(false, "Requested outbound stream without message")
        }

        self.num_requested_outbound_streams -= 1;

        if let ProtocolStatus::Unknown = self.protocol_status {
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

        if let ProtocolStatus::Unknown = self.protocol_status {
            // Upon the first successfully negotiated substream, we know that the
            // remote is configured with the same protocol name and we want
            // the behaviour to add this peer to the routing table, if possible.
            self.protocol_status = ProtocolStatus::Confirmed;
        }

        if self.inbound_substreams.len() == MAX_NUM_SUBSTREAMS {
            if let Some(s) = self.inbound_substreams.iter_mut().find(|s| {
                matches!(
                    s,
                    // An inbound substream waiting to be reused.
                    InboundSubstreamState::WaitingMessage { first: false, .. }
                )
            }) {
                *s = InboundSubstreamState::Cancelled;
                log::debug!(
                    "New inbound substream to {:?} exceeds inbound substream limit. \
                    Removed older substream waiting to be reused.",
                    self.remote_peer_id,
                )
            } else {
                log::warn!(
                    "New inbound substream to {:?} exceeds inbound substream limit. \
                     No older substream waiting to be reused. Dropping new substream.",
                    self.remote_peer_id,
                );
                return;
            }
        }

        let connec_unique_id = self.next_connec_unique_id;
        self.next_connec_unique_id.0 += 1;
        self.inbound_substreams
            .push(InboundSubstreamState::WaitingMessage {
                first: true,
                connection_id: connec_unique_id,
                substream: protocol,
            });
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError {
            info: (), error, ..
        }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        // TODO: cache the fact that the remote doesn't support kademlia at all, so that we don't
        //       continue trying

        if let Some((_, Some(query_id))) = self.pending_messages.pop_front() {
            self.outbound_substreams
                .push(OutboundSubstreamState::ReportError(error.into(), query_id));
        }

        self.num_requested_outbound_streams -= 1;
    }
}

impl ConnectionHandler for KademliaHandler {
    type FromBehaviour = KademliaHandlerIn;
    type ToBehaviour = KademliaHandlerEvent;
    type Error = io::Error; // TODO: better error type?
    type InboundProtocol = Either<KademliaProtocolConfig, upgrade::DeniedUpgrade>;
    type OutboundProtocol = KademliaProtocolConfig;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        match self.mode {
            Mode::Server => SubstreamProtocol::new(Either::Left(self.protocol_config.clone()), ()),
            Mode::Client => SubstreamProtocol::new(Either::Right(upgrade::DeniedUpgrade), ()),
        }
    }

    fn on_behaviour_event(&mut self, message: KademliaHandlerIn) {
        match message {
            KademliaHandlerIn::Reset(request_id) => {
                if let Some(state) = self
                    .inbound_substreams
                    .iter_mut()
                    .find(|state| match state {
                        InboundSubstreamState::WaitingBehaviour(conn_id, _, _) => {
                            conn_id == &request_id.connec_unique_id
                        }
                        _ => false,
                    })
                {
                    state.close();
                }
            }
            KademliaHandlerIn::FindNodeReq { key, query_id } => {
                let msg = KadRequestMsg::FindNode { key };
                self.pending_messages.push_back((msg, Some(query_id)));
            }
            KademliaHandlerIn::FindNodeRes {
                closer_peers,
                request_id,
            } => self.answer_pending_request(request_id, KadResponseMsg::FindNode { closer_peers }),
            KademliaHandlerIn::GetProvidersReq { key, query_id } => {
                let msg = KadRequestMsg::GetProviders { key };
                self.pending_messages.push_back((msg, Some(query_id)));
            }
            KademliaHandlerIn::GetProvidersRes {
                closer_peers,
                provider_peers,
                request_id,
            } => self.answer_pending_request(
                request_id,
                KadResponseMsg::GetProviders {
                    closer_peers,
                    provider_peers,
                },
            ),
            KademliaHandlerIn::AddProvider { key, provider } => {
                let msg = KadRequestMsg::AddProvider { key, provider };
                self.pending_messages.push_back((msg, None));
            }
            KademliaHandlerIn::GetRecord { key, query_id } => {
                let msg = KadRequestMsg::GetValue { key };
                self.pending_messages.push_back((msg, Some(query_id)));
            }
            KademliaHandlerIn::PutRecord { record, query_id } => {
                let msg = KadRequestMsg::PutValue { record };
                self.pending_messages.push_back((msg, Some(query_id)));
            }
            KademliaHandlerIn::GetRecordRes {
                record,
                closer_peers,
                request_id,
            } => {
                self.answer_pending_request(
                    request_id,
                    KadResponseMsg::GetValue {
                        record,
                        closer_peers,
                    },
                );
            }
            KademliaHandlerIn::PutRecordRes {
                key,
                request_id,
                value,
            } => {
                self.answer_pending_request(request_id, KadResponseMsg::PutValue { key, value });
            }
            KademliaHandlerIn::ReconfigureMode { new_mode } => {
                let peer = self.remote_peer_id;

                match &self.endpoint {
                    ConnectedPoint::Dialer { .. } => {
                        log::debug!(
                            "Now operating in {new_mode}-mode on outbound connection with {peer}"
                        )
                    }
                    ConnectedPoint::Listener { local_addr, .. } => {
                        log::debug!("Now operating in {new_mode}-mode on inbound connection with {peer} assuming that one of our external addresses routes to {local_addr}")
                    }
                }

                self.mode = new_mode;
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
            Self::ToBehaviour,
            Self::Error,
        >,
    > {
        if let ProtocolStatus::Confirmed = self.protocol_status {
            self.protocol_status = ProtocolStatus::Reported;
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                KademliaHandlerEvent::ProtocolConfirmed {
                    endpoint: self.endpoint.clone(),
                },
            ));
        }

        if let Poll::Ready(Some(event)) = self.outbound_substreams.poll_next_unpin(cx) {
            return Poll::Ready(event);
        }

        if let Poll::Ready(Some(event)) = self.inbound_substreams.poll_next_unpin(cx) {
            return Poll::Ready(event);
        }

        let num_in_progress_outbound_substreams =
            self.outbound_substreams.len() + self.num_requested_outbound_streams;
        if num_in_progress_outbound_substreams < MAX_NUM_SUBSTREAMS
            && self.num_requested_outbound_streams < self.pending_messages.len()
        {
            self.num_requested_outbound_streams += 1;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(self.protocol_config.clone(), ()),
            });
        }

        let no_streams = self.outbound_substreams.is_empty() && self.inbound_substreams.is_empty();
        self.keep_alive = match (no_streams, self.keep_alive) {
            // No open streams. Preserve the existing idle timeout.
            (true, k @ KeepAlive::Until(_)) => k,
            // No open streams. Set idle timeout.
            (true, _) => KeepAlive::Until(Instant::now() + self.idle_timeout),
            // Keep alive for open streams.
            (false, _) => KeepAlive::Yes,
        };

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
            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::ListenUpgradeError(_)
            | ConnectionEvent::LocalProtocolsChange(_) => {}
            ConnectionEvent::RemoteProtocolsChange(change) => {
                let dirty = self.remote_supported_protocols.on_protocols_change(change);

                if dirty {
                    let remote_supports_our_kademlia_protocols = self
                        .remote_supported_protocols
                        .iter()
                        .any(|p| self.protocol_config.protocol_names().contains(p));

                    match (remote_supports_our_kademlia_protocols, self.protocol_status) {
                        (true, ProtocolStatus::Confirmed | ProtocolStatus::Reported) => {}
                        (true, _) => {
                            log::debug!(
                                "Remote {} now supports our kademlia protocol on connection {}",
                                self.remote_peer_id,
                                self.connection_id,
                            );

                            self.protocol_status = ProtocolStatus::Confirmed;
                        }
                        (false, ProtocolStatus::Confirmed | ProtocolStatus::Reported) => {
                            log::debug!(
                                "Remote {} no longer supports our kademlia protocol on connection {}",
                                self.remote_peer_id,
                                self.connection_id,
                            );

                            self.protocol_status = ProtocolStatus::NotSupported;
                        }
                        (false, _) => {}
                    }
                }
            }
        }
    }
}

impl KademliaHandler {
    fn answer_pending_request(&mut self, request_id: KademliaRequestId, mut msg: KadResponseMsg) {
        for state in self.inbound_substreams.iter_mut() {
            match state.try_answer_with(request_id, msg) {
                Ok(()) => return,
                Err(m) => {
                    msg = m;
                }
            }
        }

        debug_assert!(false, "Cannot find inbound substream for {request_id:?}")
    }
}

impl futures::Stream for OutboundSubstreamState {
    type Item = ConnectionHandlerEvent<KademliaProtocolConfig, (), KademliaHandlerEvent, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            match std::mem::replace(this, OutboundSubstreamState::Poisoned) {
                OutboundSubstreamState::PendingSend(mut substream, msg, query_id) => {
                    match substream.poll_ready_unpin(cx) {
                        Poll::Ready(Ok(())) => match substream.start_send_unpin(msg) {
                            Ok(()) => {
                                *this = OutboundSubstreamState::PendingFlush(substream, query_id);
                            }
                            Err(error) => {
                                *this = OutboundSubstreamState::Done;
                                let event = query_id.map(|query_id| {
                                    ConnectionHandlerEvent::NotifyBehaviour(
                                        KademliaHandlerEvent::QueryError {
                                            error: KademliaHandlerQueryErr::Io(error),
                                            query_id,
                                        },
                                    )
                                });

                                return Poll::Ready(event);
                            }
                        },
                        Poll::Pending => {
                            *this = OutboundSubstreamState::PendingSend(substream, msg, query_id);
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(error)) => {
                            *this = OutboundSubstreamState::Done;
                            let event = query_id.map(|query_id| {
                                ConnectionHandlerEvent::NotifyBehaviour(
                                    KademliaHandlerEvent::QueryError {
                                        error: KademliaHandlerQueryErr::Io(error),
                                        query_id,
                                    },
                                )
                            });

                            return Poll::Ready(event);
                        }
                    }
                }
                OutboundSubstreamState::PendingFlush(mut substream, query_id) => {
                    match substream.poll_flush_unpin(cx) {
                        Poll::Ready(Ok(())) => {
                            if let Some(query_id) = query_id {
                                *this = OutboundSubstreamState::WaitingAnswer(substream, query_id);
                            } else {
                                *this = OutboundSubstreamState::Closing(substream);
                            }
                        }
                        Poll::Pending => {
                            *this = OutboundSubstreamState::PendingFlush(substream, query_id);
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(error)) => {
                            *this = OutboundSubstreamState::Done;
                            let event = query_id.map(|query_id| {
                                ConnectionHandlerEvent::NotifyBehaviour(
                                    KademliaHandlerEvent::QueryError {
                                        error: KademliaHandlerQueryErr::Io(error),
                                        query_id,
                                    },
                                )
                            });

                            return Poll::Ready(event);
                        }
                    }
                }
                OutboundSubstreamState::WaitingAnswer(mut substream, query_id) => {
                    match substream.poll_next_unpin(cx) {
                        Poll::Ready(Some(Ok(msg))) => {
                            *this = OutboundSubstreamState::Closing(substream);
                            let event = process_kad_response(msg, query_id);

                            return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                                event,
                            )));
                        }
                        Poll::Pending => {
                            *this = OutboundSubstreamState::WaitingAnswer(substream, query_id);
                            return Poll::Pending;
                        }
                        Poll::Ready(Some(Err(error))) => {
                            *this = OutboundSubstreamState::Done;
                            let event = KademliaHandlerEvent::QueryError {
                                error: KademliaHandlerQueryErr::Io(error),
                                query_id,
                            };

                            return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                                event,
                            )));
                        }
                        Poll::Ready(None) => {
                            *this = OutboundSubstreamState::Done;
                            let event = KademliaHandlerEvent::QueryError {
                                error: KademliaHandlerQueryErr::Io(
                                    io::ErrorKind::UnexpectedEof.into(),
                                ),
                                query_id,
                            };

                            return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                                event,
                            )));
                        }
                    }
                }
                OutboundSubstreamState::ReportError(error, query_id) => {
                    *this = OutboundSubstreamState::Done;
                    let event = KademliaHandlerEvent::QueryError { error, query_id };

                    return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(event)));
                }
                OutboundSubstreamState::Closing(mut stream) => match stream.poll_close_unpin(cx) {
                    Poll::Ready(Ok(())) | Poll::Ready(Err(_)) => return Poll::Ready(None),
                    Poll::Pending => {
                        *this = OutboundSubstreamState::Closing(stream);
                        return Poll::Pending;
                    }
                },
                OutboundSubstreamState::Done => {
                    *this = OutboundSubstreamState::Done;
                    return Poll::Ready(None);
                }
                OutboundSubstreamState::Poisoned => unreachable!(),
            }
        }
    }
}

impl futures::Stream for InboundSubstreamState {
    type Item = ConnectionHandlerEvent<KademliaProtocolConfig, (), KademliaHandlerEvent, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            match std::mem::replace(
                this,
                Self::Poisoned {
                    phantom: PhantomData,
                },
            ) {
                InboundSubstreamState::WaitingMessage {
                    first,
                    connection_id,
                    mut substream,
                } => match substream.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok(KadRequestMsg::Ping))) => {
                        log::warn!("Kademlia PING messages are unsupported");

                        *this = InboundSubstreamState::Closing(substream);
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::FindNode { key }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            KademliaHandlerEvent::FindNodeReq {
                                key,
                                request_id: KademliaRequestId {
                                    connec_unique_id: connection_id,
                                },
                            },
                        )));
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::GetProviders { key }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            KademliaHandlerEvent::GetProvidersReq {
                                key,
                                request_id: KademliaRequestId {
                                    connec_unique_id: connection_id,
                                },
                            },
                        )));
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::AddProvider { key, provider }))) => {
                        *this = InboundSubstreamState::WaitingMessage {
                            first: false,
                            connection_id,
                            substream,
                        };
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            KademliaHandlerEvent::AddProvider { key, provider },
                        )));
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::GetValue { key }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            KademliaHandlerEvent::GetRecord {
                                key,
                                request_id: KademliaRequestId {
                                    connec_unique_id: connection_id,
                                },
                            },
                        )));
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::PutValue { record }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            KademliaHandlerEvent::PutRecord {
                                record,
                                request_id: KademliaRequestId {
                                    connec_unique_id: connection_id,
                                },
                            },
                        )));
                    }
                    Poll::Pending => {
                        *this = InboundSubstreamState::WaitingMessage {
                            first,
                            connection_id,
                            substream,
                        };
                        return Poll::Pending;
                    }
                    Poll::Ready(None) => {
                        return Poll::Ready(None);
                    }
                    Poll::Ready(Some(Err(e))) => {
                        trace!("Inbound substream error: {:?}", e);
                        return Poll::Ready(None);
                    }
                },
                InboundSubstreamState::WaitingBehaviour(id, substream, _) => {
                    *this = InboundSubstreamState::WaitingBehaviour(
                        id,
                        substream,
                        Some(cx.waker().clone()),
                    );

                    return Poll::Pending;
                }
                InboundSubstreamState::PendingSend(id, mut substream, msg) => {
                    match substream.poll_ready_unpin(cx) {
                        Poll::Ready(Ok(())) => match substream.start_send_unpin(msg) {
                            Ok(()) => {
                                *this = InboundSubstreamState::PendingFlush(id, substream);
                            }
                            Err(_) => return Poll::Ready(None),
                        },
                        Poll::Pending => {
                            *this = InboundSubstreamState::PendingSend(id, substream, msg);
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(_)) => return Poll::Ready(None),
                    }
                }
                InboundSubstreamState::PendingFlush(id, mut substream) => {
                    match substream.poll_flush_unpin(cx) {
                        Poll::Ready(Ok(())) => {
                            *this = InboundSubstreamState::WaitingMessage {
                                first: false,
                                connection_id: id,
                                substream,
                            };
                        }
                        Poll::Pending => {
                            *this = InboundSubstreamState::PendingFlush(id, substream);
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(_)) => return Poll::Ready(None),
                    }
                }
                InboundSubstreamState::Closing(mut stream) => match stream.poll_close_unpin(cx) {
                    Poll::Ready(Ok(())) | Poll::Ready(Err(_)) => return Poll::Ready(None),
                    Poll::Pending => {
                        *this = InboundSubstreamState::Closing(stream);
                        return Poll::Pending;
                    }
                },
                InboundSubstreamState::Poisoned { .. } => unreachable!(),
                InboundSubstreamState::Cancelled => return Poll::Ready(None),
            }
        }
    }
}

/// Process a Kademlia message that's supposed to be a response to one of our requests.
fn process_kad_response(event: KadResponseMsg, query_id: QueryId) -> KademliaHandlerEvent {
    // TODO: must check that the response corresponds to the request
    match event {
        KadResponseMsg::Pong => {
            // We never send out pings.
            KademliaHandlerEvent::QueryError {
                error: KademliaHandlerQueryErr::UnexpectedMessage,
                query_id,
            }
        }
        KadResponseMsg::FindNode { closer_peers } => KademliaHandlerEvent::FindNodeRes {
            closer_peers,
            query_id,
        },
        KadResponseMsg::GetProviders {
            closer_peers,
            provider_peers,
        } => KademliaHandlerEvent::GetProvidersRes {
            closer_peers,
            provider_peers,
            query_id,
        },
        KadResponseMsg::GetValue {
            record,
            closer_peers,
        } => KademliaHandlerEvent::GetRecordRes {
            record,
            closer_peers,
            query_id,
        },
        KadResponseMsg::PutValue { key, value, .. } => KademliaHandlerEvent::PutRecordRes {
            key,
            value,
            query_id,
        },
    }
}
