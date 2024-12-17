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

use std::{
    collections::VecDeque,
    error, fmt, io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};

use either::Either;
use futures::{channel::oneshot, prelude::*, stream::SelectAll};
use libp2p_core::{upgrade, ConnectedPoint};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    handler::{ConnectionEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound},
    ConnectionHandler, ConnectionHandlerEvent, Stream, StreamUpgradeError, SubstreamProtocol,
    SupportedProtocols,
};

use crate::{
    behaviour::Mode,
    protocol::{
        KadInStreamSink, KadOutStreamSink, KadPeer, KadRequestMsg, KadResponseMsg, ProtocolConfig,
    },
    record::{self, Record},
    QueryId,
};

const MAX_NUM_STREAMS: usize = 32;

/// Protocol handler that manages substreams for the Kademlia protocol
/// on a single connection with a peer.
///
/// The handler will automatically open a Kademlia substream with the remote for each request we
/// make.
///
/// It also handles requests made by the remote.
pub struct Handler {
    /// Configuration of the wire protocol.
    protocol_config: ProtocolConfig,

    /// In client mode, we don't accept inbound substreams.
    mode: Mode,

    /// Next unique ID of a connection.
    next_connec_unique_id: UniqueConnecId,

    /// List of active outbound streams.
    outbound_substreams:
        futures_bounded::FuturesTupleSet<io::Result<Option<KadResponseMsg>>, QueryId>,

    /// Contains one [`oneshot::Sender`] per outbound stream that we have requested.
    pending_streams:
        VecDeque<oneshot::Sender<Result<KadOutStreamSink<Stream>, StreamUpgradeError<io::Error>>>>,

    /// List of outbound substreams that are waiting to become active next.
    /// Contains the request we want to send, and the user data if we expect an answer.
    pending_messages: VecDeque<(KadRequestMsg, QueryId)>,

    /// List of active inbound substreams with the state they are in.
    inbound_substreams: SelectAll<InboundSubstreamState>,

    /// The connected endpoint of the connection that the handler
    /// is associated with.
    endpoint: ConnectedPoint,

    /// The [`PeerId`] of the remote.
    remote_peer_id: PeerId,

    /// The current state of protocol confirmation.
    protocol_status: Option<ProtocolStatus>,

    remote_supported_protocols: SupportedProtocols,
}

/// The states of protocol confirmation that a connection
/// handler transitions through.
#[derive(Debug, Copy, Clone, PartialEq)]
struct ProtocolStatus {
    /// Whether the remote node supports one of our kademlia protocols.
    supported: bool,
    /// Whether we reported the state to the behaviour.
    reported: bool,
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
    /// Waiting for the behaviour to send a [`HandlerIn`] event containing the response.
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
        id: RequestId,
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
pub enum HandlerEvent {
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
        request_id: RequestId,
    },

    /// Response to an `HandlerIn::FindNodeReq`.
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
        key: record::Key,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: RequestId,
    },

    /// Response to an `HandlerIn::GetProvidersReq`.
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
        error: HandlerQueryErr,
        /// The user data passed to the query.
        query_id: QueryId,
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
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: RequestId,
    },

    /// Response to a `HandlerIn::GetRecord`.
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
        request_id: RequestId,
    },

    /// Response to a request to store a record.
    PutRecordRes {
        /// The key of the stored record.
        key: record::Key,
        /// The value of the stored record.
        value: Vec<u8>,
        /// The user data passed to the `PutValue`.
        query_id: QueryId,
    },
}

/// Error that can happen when requesting an RPC query.
#[derive(Debug)]
pub enum HandlerQueryErr {
    /// Received an answer that doesn't correspond to the request.
    UnexpectedMessage,
    /// I/O error in the substream.
    Io(io::Error),
}

impl fmt::Display for HandlerQueryErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandlerQueryErr::UnexpectedMessage => {
                write!(
                    f,
                    "Remote answered our Kademlia RPC query with the wrong message type"
                )
            }
            HandlerQueryErr::Io(err) => {
                write!(f, "I/O error during a Kademlia RPC query: {err}")
            }
        }
    }
}

impl error::Error for HandlerQueryErr {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            HandlerQueryErr::UnexpectedMessage => None,
            HandlerQueryErr::Io(err) => Some(err),
        }
    }
}

/// Event to send to the handler.
#[derive(Debug)]
pub enum HandlerIn {
    /// Resets the (sub)stream associated with the given request ID,
    /// thus signaling an error to the remote.
    ///
    /// Explicitly resetting the (sub)stream associated with a request
    /// can be used as an alternative to letting requests simply time
    /// out on the remote peer, thus potentially avoiding some delay
    /// for the query on the remote.
    Reset(RequestId),

    /// Change the connection to the specified mode.
    ReconfigureMode { new_mode: Mode },

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// Identifier of the node.
        key: Vec<u8>,
        /// ID of the query that generated this request.
        query_id: QueryId,
    },

    /// Response to a `FindNodeReq`.
    FindNodeRes {
        /// Results of the request.
        closer_peers: Vec<KadPeer>,
        /// Identifier of the request that was made by the remote.
        ///
        /// It is a logic error to use an id of the handler of a different node.
        request_id: RequestId,
    },

    /// Same as `FindNodeReq`, but should also return the entries of the local providers list for
    /// this key.
    GetProvidersReq {
        /// Identifier being searched.
        key: record::Key,
        /// ID of the query that generated this request.
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
        request_id: RequestId,
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
        /// ID of the query that generated this request.
        query_id: QueryId,
    },

    /// Request to retrieve a record from the DHT.
    GetRecord {
        /// The key of the record.
        key: record::Key,
        /// ID of the query that generated this request.
        query_id: QueryId,
    },

    /// Response to a `GetRecord` request.
    GetRecordRes {
        /// The value that might have been found in our storage.
        record: Option<Record>,
        /// Nodes that are closer to the key we were searching for.
        closer_peers: Vec<KadPeer>,
        /// Identifier of the request that was made by the remote.
        request_id: RequestId,
    },

    /// Put a value into the dht records.
    PutRecord {
        record: Record,
        /// ID of the query that generated this request.
        query_id: QueryId,
    },

    /// Response to a `PutRecord`.
    PutRecordRes {
        /// Key of the value that was put.
        key: record::Key,
        /// Value that was put.
        value: Vec<u8>,
        /// Identifier of the request that was made by the remote.
        request_id: RequestId,
    },
}

/// Unique identifier for a request. Must be passed back in order to answer a request from
/// the remote.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct RequestId {
    /// Unique identifier for an incoming connection.
    connec_unique_id: UniqueConnecId,
}

/// Unique identifier for a connection.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct UniqueConnecId(u64);

impl Handler {
    pub fn new(
        protocol_config: ProtocolConfig,
        endpoint: ConnectedPoint,
        remote_peer_id: PeerId,
        mode: Mode,
    ) -> Self {
        match &endpoint {
            ConnectedPoint::Dialer { .. } => {
                tracing::debug!(
                    peer=%remote_peer_id,
                    mode=%mode,
                    "New outbound connection"
                );
            }
            ConnectedPoint::Listener { .. } => {
                tracing::debug!(
                    peer=%remote_peer_id,
                    mode=%mode,
                    "New inbound connection"
                );
            }
        }

        Handler {
            protocol_config,
            mode,
            endpoint,
            remote_peer_id,
            next_connec_unique_id: UniqueConnecId(0),
            inbound_substreams: Default::default(),
            outbound_substreams: futures_bounded::FuturesTupleSet::new(
                Duration::from_secs(10),
                MAX_NUM_STREAMS,
            ),
            pending_streams: Default::default(),
            pending_messages: Default::default(),
            protocol_status: None,
            remote_supported_protocols: Default::default(),
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: stream,
            info: (),
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        if let Some(sender) = self.pending_streams.pop_front() {
            let _ = sender.send(Ok(stream));
        }

        if self.protocol_status.is_none() {
            // Upon the first successfully negotiated substream, we know that the
            // remote is configured with the same protocol name and we want
            // the behaviour to add this peer to the routing table, if possible.
            self.protocol_status = Some(ProtocolStatus {
                supported: true,
                reported: false,
            });
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
        // is a `Infallible`.
        let protocol = match protocol {
            future::Either::Left(p) => p,
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            future::Either::Right(p) => libp2p_core::util::unreachable(p),
        };

        if self.protocol_status.is_none() {
            // Upon the first successfully negotiated substream, we know that the
            // remote is configured with the same protocol name and we want
            // the behaviour to add this peer to the routing table, if possible.
            self.protocol_status = Some(ProtocolStatus {
                supported: true,
                reported: false,
            });
        }

        if self.inbound_substreams.len() == MAX_NUM_STREAMS {
            if let Some(s) = self.inbound_substreams.iter_mut().find(|s| {
                matches!(
                    s,
                    // An inbound substream waiting to be reused.
                    InboundSubstreamState::WaitingMessage { first: false, .. }
                )
            }) {
                *s = InboundSubstreamState::Cancelled;
                tracing::debug!(
                    peer=?self.remote_peer_id,
                    "New inbound substream to peer exceeds inbound substream limit. \
                    Removed older substream waiting to be reused."
                )
            } else {
                tracing::warn!(
                    peer=?self.remote_peer_id,
                    "New inbound substream to peer exceeds inbound substream limit. \
                     No older substream waiting to be reused. Dropping new substream."
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

    /// Takes the given [`KadRequestMsg`] and composes it into an outbound request-response protocol
    /// handshake using a [`oneshot::channel`].
    fn queue_new_stream(&mut self, id: QueryId, msg: KadRequestMsg) {
        let (sender, receiver) = oneshot::channel();

        self.pending_streams.push_back(sender);
        let result = self.outbound_substreams.try_push(
            async move {
                let mut stream = receiver
                    .await
                    .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?
                    .map_err(|e| match e {
                        StreamUpgradeError::Timeout => io::ErrorKind::TimedOut.into(),
                        StreamUpgradeError::Apply(e) => e,
                        StreamUpgradeError::NegotiationFailed => io::Error::new(
                            io::ErrorKind::ConnectionRefused,
                            "protocol not supported",
                        ),
                        StreamUpgradeError::Io(e) => e,
                    })?;

                let has_answer = !matches!(msg, KadRequestMsg::AddProvider { .. });

                stream.send(msg).await?;
                stream.close().await?;

                if !has_answer {
                    return Ok(None);
                }

                let msg = stream.next().await.ok_or(io::ErrorKind::UnexpectedEof)??;

                Ok(Some(msg))
            },
            id,
        );

        debug_assert!(
            result.is_ok(),
            "Expected to not create more streams than allowed"
        );
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = HandlerIn;
    type ToBehaviour = HandlerEvent;
    type InboundProtocol = Either<ProtocolConfig, upgrade::DeniedUpgrade>;
    type OutboundProtocol = ProtocolConfig;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        match self.mode {
            Mode::Server => SubstreamProtocol::new(Either::Left(self.protocol_config.clone()), ()),
            Mode::Client => SubstreamProtocol::new(Either::Right(upgrade::DeniedUpgrade), ()),
        }
    }

    fn on_behaviour_event(&mut self, message: HandlerIn) {
        match message {
            HandlerIn::Reset(request_id) => {
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
            HandlerIn::FindNodeReq { key, query_id } => {
                let msg = KadRequestMsg::FindNode { key };
                self.pending_messages.push_back((msg, query_id));
            }
            HandlerIn::FindNodeRes {
                closer_peers,
                request_id,
            } => self.answer_pending_request(request_id, KadResponseMsg::FindNode { closer_peers }),
            HandlerIn::GetProvidersReq { key, query_id } => {
                let msg = KadRequestMsg::GetProviders { key };
                self.pending_messages.push_back((msg, query_id));
            }
            HandlerIn::GetProvidersRes {
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
            HandlerIn::AddProvider {
                key,
                provider,
                query_id,
            } => {
                let msg = KadRequestMsg::AddProvider { key, provider };
                self.pending_messages.push_back((msg, query_id));
            }
            HandlerIn::GetRecord { key, query_id } => {
                let msg = KadRequestMsg::GetValue { key };
                self.pending_messages.push_back((msg, query_id));
            }
            HandlerIn::PutRecord { record, query_id } => {
                let msg = KadRequestMsg::PutValue { record };
                self.pending_messages.push_back((msg, query_id));
            }
            HandlerIn::GetRecordRes {
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
            HandlerIn::PutRecordRes {
                key,
                request_id,
                value,
            } => {
                self.answer_pending_request(request_id, KadResponseMsg::PutValue { key, value });
            }
            HandlerIn::ReconfigureMode { new_mode } => {
                let peer = self.remote_peer_id;

                match &self.endpoint {
                    ConnectedPoint::Dialer { .. } => {
                        tracing::debug!(
                            %peer,
                            mode=%new_mode,
                            "Changed mode on outbound connection"
                        )
                    }
                    ConnectedPoint::Listener { local_addr, .. } => {
                        tracing::debug!(
                            %peer,
                            mode=%new_mode,
                            local_address=%local_addr,
                            "Changed mode on inbound connection assuming that one of our external addresses routes to the local address")
                    }
                }

                self.mode = new_mode;
            }
        }
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        loop {
            match &mut self.protocol_status {
                Some(status) if !status.reported => {
                    status.reported = true;
                    let event = if status.supported {
                        HandlerEvent::ProtocolConfirmed {
                            endpoint: self.endpoint.clone(),
                        }
                    } else {
                        HandlerEvent::ProtocolNotSupported {
                            endpoint: self.endpoint.clone(),
                        }
                    };

                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
                }
                _ => {}
            }

            match self.outbound_substreams.poll_unpin(cx) {
                Poll::Ready((Ok(Ok(Some(response))), query_id)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        process_kad_response(response, query_id),
                    ))
                }
                Poll::Ready((Ok(Ok(None)), _)) => {
                    continue;
                }
                Poll::Ready((Ok(Err(e)), query_id)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        HandlerEvent::QueryError {
                            error: HandlerQueryErr::Io(e),
                            query_id,
                        },
                    ))
                }
                Poll::Ready((Err(_timeout), query_id)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        HandlerEvent::QueryError {
                            error: HandlerQueryErr::Io(io::ErrorKind::TimedOut.into()),
                            query_id,
                        },
                    ))
                }
                Poll::Pending => {}
            }

            if let Poll::Ready(Some(event)) = self.inbound_substreams.poll_next_unpin(cx) {
                return Poll::Ready(event);
            }

            if self.outbound_substreams.len() < MAX_NUM_STREAMS {
                if let Some((msg, id)) = self.pending_messages.pop_front() {
                    self.queue_new_stream(id, msg);
                    return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(self.protocol_config.clone(), ()),
                    });
                }
            }

            return Poll::Pending;
        }
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
            ConnectionEvent::DialUpgradeError(ev) => {
                if let Some(sender) = self.pending_streams.pop_front() {
                    let _ = sender.send(Err(ev.error));
                }
            }
            ConnectionEvent::RemoteProtocolsChange(change) => {
                let dirty = self.remote_supported_protocols.on_protocols_change(change);

                if dirty {
                    let remote_supports_our_kademlia_protocols = self
                        .remote_supported_protocols
                        .iter()
                        .any(|p| self.protocol_config.protocol_names().contains(p));

                    self.protocol_status = Some(compute_new_protocol_status(
                        remote_supports_our_kademlia_protocols,
                        self.protocol_status,
                    ))
                }
            }
            _ => {}
        }
    }
}

fn compute_new_protocol_status(
    now_supported: bool,
    current_status: Option<ProtocolStatus>,
) -> ProtocolStatus {
    let current_status = match current_status {
        None => {
            return ProtocolStatus {
                supported: now_supported,
                reported: false,
            }
        }
        Some(current) => current,
    };

    if now_supported == current_status.supported {
        return ProtocolStatus {
            supported: now_supported,
            reported: true,
        };
    }

    if now_supported {
        tracing::debug!("Remote now supports our kademlia protocol");
    } else {
        tracing::debug!("Remote no longer supports our kademlia protocol");
    }

    ProtocolStatus {
        supported: now_supported,
        reported: false,
    }
}

impl Handler {
    fn answer_pending_request(&mut self, request_id: RequestId, mut msg: KadResponseMsg) {
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

impl futures::Stream for InboundSubstreamState {
    type Item = ConnectionHandlerEvent<ProtocolConfig, (), HandlerEvent>;

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
                        tracing::warn!("Kademlia PING messages are unsupported");

                        *this = InboundSubstreamState::Closing(substream);
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::FindNode { key }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            HandlerEvent::FindNodeReq {
                                key,
                                request_id: RequestId {
                                    connec_unique_id: connection_id,
                                },
                            },
                        )));
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::GetProviders { key }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            HandlerEvent::GetProvidersReq {
                                key,
                                request_id: RequestId {
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
                            HandlerEvent::AddProvider { key, provider },
                        )));
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::GetValue { key }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            HandlerEvent::GetRecord {
                                key,
                                request_id: RequestId {
                                    connec_unique_id: connection_id,
                                },
                            },
                        )));
                    }
                    Poll::Ready(Some(Ok(KadRequestMsg::PutValue { record }))) => {
                        *this =
                            InboundSubstreamState::WaitingBehaviour(connection_id, substream, None);
                        return Poll::Ready(Some(ConnectionHandlerEvent::NotifyBehaviour(
                            HandlerEvent::PutRecord {
                                record,
                                request_id: RequestId {
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
                        tracing::trace!("Inbound substream error: {:?}", e);
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
fn process_kad_response(event: KadResponseMsg, query_id: QueryId) -> HandlerEvent {
    // TODO: must check that the response corresponds to the request
    match event {
        KadResponseMsg::Pong => {
            // We never send out pings.
            HandlerEvent::QueryError {
                error: HandlerQueryErr::UnexpectedMessage,
                query_id,
            }
        }
        KadResponseMsg::FindNode { closer_peers } => HandlerEvent::FindNodeRes {
            closer_peers,
            query_id,
        },
        KadResponseMsg::GetProviders {
            closer_peers,
            provider_peers,
        } => HandlerEvent::GetProvidersRes {
            closer_peers,
            provider_peers,
            query_id,
        },
        KadResponseMsg::GetValue {
            record,
            closer_peers,
        } => HandlerEvent::GetRecordRes {
            record,
            closer_peers,
            query_id,
        },
        KadResponseMsg::PutValue { key, value, .. } => HandlerEvent::PutRecordRes {
            key,
            value,
            query_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::{Arbitrary, Gen};
    use tracing_subscriber::EnvFilter;

    use super::*;

    impl Arbitrary for ProtocolStatus {
        fn arbitrary(g: &mut Gen) -> Self {
            Self {
                supported: bool::arbitrary(g),
                reported: bool::arbitrary(g),
            }
        }
    }

    #[test]
    fn compute_next_protocol_status_test() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .try_init();

        fn prop(now_supported: bool, current: Option<ProtocolStatus>) {
            let new = compute_new_protocol_status(now_supported, current);

            match current {
                None => {
                    assert!(!new.reported);
                    assert_eq!(new.supported, now_supported);
                }
                Some(current) => {
                    if current.supported == now_supported {
                        assert!(new.reported);
                    } else {
                        assert!(!new.reported);
                    }

                    assert_eq!(new.supported, now_supported);
                }
            }
        }

        quickcheck::quickcheck(prop as fn(_, _))
    }
}
