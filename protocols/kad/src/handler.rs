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

use crate::protocol::{
    KadInStreamSink, KadOutStreamSink, KadPeer, KadRequestMsg, KadResponseMsg,
    KademliaProtocolConfig,
};
use crate::record::Record;
use futures::prelude::*;
use libp2p_core::protocols_handler::{
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr
};
use libp2p_core::{upgrade, either::EitherOutput, InboundUpgrade, OutboundUpgrade, upgrade::Negotiated};
use multihash::Multihash;
use std::{borrow::Cow, error, fmt, io, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::Instant;

/// Protocol handler that handles Kademlia communications with the remote.
///
/// The handler will automatically open a Kademlia substream with the remote for each request we
/// make.
///
/// It also handles requests made by the remote.
pub struct KademliaHandler<TSubstream, TUserData>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Configuration for the Kademlia protocol.
    config: KademliaProtocolConfig,

    /// If false, we always refuse incoming Kademlia substreams.
    allow_listening: bool,

    /// Next unique ID of a connection.
    next_connec_unique_id: UniqueConnecId,

    /// List of active substreams with the state they are in.
    substreams: Vec<SubstreamState<Negotiated<TSubstream>, TUserData>>,

    /// Until when to keep the connection alive.
    keep_alive: KeepAlive,
}

/// State of an active substream, opened either by us or by the remote.
enum SubstreamState<TSubstream, TUserData>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// We haven't started opening the outgoing substream yet.
    /// Contains the request we want to send, and the user data if we expect an answer.
    OutPendingOpen(KadRequestMsg, Option<TUserData>),
    /// Waiting to send a message to the remote.
    OutPendingSend(
        KadOutStreamSink<TSubstream>,
        KadRequestMsg,
        Option<TUserData>,
    ),
    /// Waiting to send a message to the remote.
    /// Waiting to flush the substream so that the data arrives to the remote.
    OutPendingFlush(KadOutStreamSink<TSubstream>, Option<TUserData>),
    /// Waiting for an answer back from the remote.
    // TODO: add timeout
    OutWaitingAnswer(KadOutStreamSink<TSubstream>, TUserData),
    /// An error happened on the substream and we should report the error to the user.
    OutReportError(KademliaHandlerQueryErr, TUserData),
    /// The substream is being closed.
    OutClosing(KadOutStreamSink<TSubstream>),
    /// Waiting for a request from the remote.
    InWaitingMessage(UniqueConnecId, KadInStreamSink<TSubstream>),
    /// Waiting for the user to send a `KademliaHandlerIn` event containing the response.
    InWaitingUser(UniqueConnecId, KadInStreamSink<TSubstream>),
    /// Waiting to send an answer back to the remote.
    InPendingSend(UniqueConnecId, KadInStreamSink<TSubstream>, KadResponseMsg),
    /// Waiting to flush an answer back to the remote.
    InPendingFlush(UniqueConnecId, KadInStreamSink<TSubstream>),
    /// The substream is being closed.
    InClosing(KadInStreamSink<TSubstream>),
}

impl<TSubstream, TUserData> SubstreamState<TSubstream, TUserData>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Consumes this state and tries to close the substream.
    ///
    /// If the substream is not ready to be closed, returns it back.
    fn try_close(self) -> AsyncSink<Self> {
        match self {
            SubstreamState::OutPendingOpen(_, _)
            | SubstreamState::OutReportError(_, _) => AsyncSink::Ready,
            SubstreamState::OutPendingSend(mut stream, _, _)
            | SubstreamState::OutPendingFlush(mut stream, _)
            | SubstreamState::OutWaitingAnswer(mut stream, _)
            | SubstreamState::OutClosing(mut stream) => match stream.close() {
                Ok(Async::Ready(())) | Err(_) => AsyncSink::Ready,
                Ok(Async::NotReady) => AsyncSink::NotReady(SubstreamState::OutClosing(stream)),
            },
            SubstreamState::InWaitingMessage(_, mut stream)
            | SubstreamState::InWaitingUser(_, mut stream)
            | SubstreamState::InPendingSend(_, mut stream, _)
            | SubstreamState::InPendingFlush(_, mut stream)
            | SubstreamState::InClosing(mut stream) => match stream.close() {
                Ok(Async::Ready(())) | Err(_) => AsyncSink::Ready,
                Ok(Async::NotReady) => AsyncSink::NotReady(SubstreamState::InClosing(stream)),
            },
        }
    }
}

/// Event produced by the Kademlia handler.
#[derive(Debug)]
pub enum KademliaHandlerEvent<TUserData> {
    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// The key for which to locate the closest nodes.
        key: Multihash,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: KademliaRequestId,
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
        /// Identifier being searched.
        key: Multihash,
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
        user_data: TUserData,
    },

    /// An error happened when performing a query.
    QueryError {
        /// The error that happened.
        error: KademliaHandlerQueryErr,
        /// The user data passed to the query.
        user_data: TUserData,
    },

    /// The remote indicates that this list of providers is known for this key.
    AddProvider {
        /// Key for which we should add providers.
        key: Multihash,
        /// Known provider for this key.
        provider_peer: KadPeer,
    },

    /// Request to get a value from the dht records
    GetValue {
        /// Key for which we should look in the dht
        key: Multihash,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: KademliaRequestId,
    },

    /// Response to a `KademliaHandlerIn::GetValue`.
    GetValueRes {
        /// The result is present if the key has been found
        result: Option<Record>,
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// The user data passed to the `GetValue`.
        user_data: TUserData,
    },

    /// Request to put a value in the dht records
    PutValue {
        /// The key of the record
        key: Multihash,
        /// The value of the record
        value: Vec<u8>,
        /// Identifier of the request. Needs to be passed back when answering.
        request_id: KademliaRequestId,
    },

    /// Response to a request to put a value
    PutValueRes {
        /// The key we were putting in
        key: Multihash,
        /// The user data passed to the `GetValue`.
        user_data: TUserData,
    }
}

/// Error that can happen when requesting an RPC query.
#[derive(Debug)]
pub enum KademliaHandlerQueryErr {
    /// Error while trying to perform the query.
    Upgrade(ProtocolsHandlerUpgrErr<io::Error>),
    /// Received an answer that doesn't correspond to the request.
    UnexpectedMessage,
    /// I/O error in the substream.
    Io(io::Error),
}

impl fmt::Display for KademliaHandlerQueryErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KademliaHandlerQueryErr::Upgrade(err) => {
                write!(f, "Error while performing Kademlia query: {}", err)
            },
            KademliaHandlerQueryErr::UnexpectedMessage => {
                write!(f, "Remote answered our Kademlia RPC query with the wrong message type")
            },
            KademliaHandlerQueryErr::Io(err) => {
                write!(f, "I/O error during a Kademlia RPC query: {}", err)
            },
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

impl From<ProtocolsHandlerUpgrErr<io::Error>> for KademliaHandlerQueryErr {
    #[inline]
    fn from(err: ProtocolsHandlerUpgrErr<io::Error>) -> Self {
        KademliaHandlerQueryErr::Upgrade(err)
    }
}

/// Event to send to the handler.
#[derive(Debug)]
pub enum KademliaHandlerIn<TUserData> {
    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// Identifier of the node.
        key: Multihash,
        /// Custom user data. Passed back in the out event when the results arrive.
        user_data: TUserData,
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
        key: Multihash,
        /// Custom user data. Passed back in the out event when the results arrive.
        user_data: TUserData,
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
        key: Multihash,
        /// Known provider for this key.
        provider_peer: KadPeer,
    },

    /// Request to get a node from the dht
    GetValue {
        /// The key of the value we are looking for
        key: Multihash,
        /// Custom data. Passed back in the out event when the results arrive.
        user_data: TUserData,
    },

    /// Response to a `GetValue`.
    GetValueRes {
        /// The value that might have been found in our storage.
        result: Option<Record>,
        /// Nodes that are closer to the key we were searching for.
        closer_peers: Vec<KadPeer>,
        /// Identifier of the request that was made by the remote.
        request_id: KademliaRequestId,
    },

    /// Put a value into the dht records.
    PutValue {
        /// The key of the record.
        key: Multihash,
        /// The value of the record.
        value: Vec<u8>,
        /// Custom data. Passed back in the out event when the results arrive.
        user_data: TUserData,
    },

    /// Response to a `PutValue`.
    PutValueRes {
        /// Key of the value that was put.
        key: Multihash,
        /// Value that was put.
        value: Vec<u8>,
        /// Identifier of the request that was made by the remote.
        request_id: KademliaRequestId,
    }
}

/// Unique identifier for a request. Must be passed back in order to answer a request from
/// the remote.
///
/// We don't implement `Clone` on purpose, in order to prevent users from answering the same
/// request twice.
#[derive(Debug, PartialEq, Eq)]
pub struct KademliaRequestId {
    /// Unique identifier for an incoming connection.
    connec_unique_id: UniqueConnecId,
}

/// Unique identifier for a connection.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct UniqueConnecId(u64);

impl<TSubstream, TUserData> KademliaHandler<TSubstream, TUserData>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Create a `KademliaHandler` that only allows sending messages to the remote but denying
    /// incoming connections.
    pub fn dial_only() -> Self {
        KademliaHandler::with_allow_listening(false)
    }

    /// Create a `KademliaHandler` that only allows sending messages but also receive incoming
    /// requests.
    ///
    /// The `Default` trait implementation wraps around this function.
    pub fn dial_and_listen() -> Self {
        KademliaHandler::with_allow_listening(true)
    }

    fn with_allow_listening(allow_listening: bool) -> Self {
        KademliaHandler {
            config: Default::default(),
            allow_listening,
            next_connec_unique_id: UniqueConnecId(0),
            substreams: Vec::new(),
            keep_alive: KeepAlive::Yes,
        }
    }

    /// Modifies the protocol name used on the wire. Can be used to create incompatibilities
    /// between networks on purpose.
    pub fn with_protocol_name(mut self, name: impl Into<Cow<'static, [u8]>>) -> Self {
        self.config = self.config.with_protocol_name(name);
        self
    }
}

impl<TSubstream, TUserData> Default for KademliaHandler<TSubstream, TUserData>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    #[inline]
    fn default() -> Self {
        KademliaHandler::dial_and_listen()
    }
}

impl<TSubstream, TUserData> ProtocolsHandler for KademliaHandler<TSubstream, TUserData>
where
    TSubstream: AsyncRead + AsyncWrite,
    TUserData: Clone,
{
    type InEvent = KademliaHandlerIn<TUserData>;
    type OutEvent = KademliaHandlerEvent<TUserData>;
    type Error = io::Error; // TODO: better error type?
    type Substream = TSubstream;
    type InboundProtocol = upgrade::EitherUpgrade<KademliaProtocolConfig, upgrade::DeniedUpgrade>;
    type OutboundProtocol = KademliaProtocolConfig;
    // Message of the request to send to the remote, and user data if we expect an answer.
    type OutboundOpenInfo = (KadRequestMsg, Option<TUserData>);

    #[inline]
    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        if self.allow_listening {
            SubstreamProtocol::new(self.config.clone()).map_upgrade(upgrade::EitherUpgrade::A)
        } else {
            SubstreamProtocol::new(upgrade::EitherUpgrade::B(upgrade::DeniedUpgrade))
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        (msg, user_data): Self::OutboundOpenInfo,
    ) {
        self.substreams
            .push(SubstreamState::OutPendingSend(protocol, msg, user_data));
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output,
    ) {
        // If `self.allow_listening` is false, then we produced a `DeniedUpgrade` and `protocol`
        // is a `Void`.
        let protocol = match protocol {
            EitherOutput::First(p) => p,
            EitherOutput::Second(p) => void::unreachable(p),
        };

        debug_assert!(self.allow_listening);
        let connec_unique_id = self.next_connec_unique_id;
        self.next_connec_unique_id.0 += 1;
        self.substreams
            .push(SubstreamState::InWaitingMessage(connec_unique_id, protocol));
    }

    #[inline]
    fn inject_event(&mut self, message: KademliaHandlerIn<TUserData>) {
        match message {
            KademliaHandlerIn::FindNodeReq { key, user_data } => {
                let msg = KadRequestMsg::FindNode { key };
                self.substreams.push(SubstreamState::OutPendingOpen(msg, Some(user_data.clone())));
            }
            KademliaHandlerIn::FindNodeRes {
                closer_peers,
                request_id,
            } => {
                let pos = self.substreams.iter().position(|state| match state {
                    SubstreamState::InWaitingUser(ref conn_id, _) =>
                        conn_id == &request_id.connec_unique_id,
                    _ => false,
                });

                if let Some(pos) = pos {
                    let (conn_id, substream) = match self.substreams.remove(pos) {
                        SubstreamState::InWaitingUser(conn_id, substream) => (conn_id, substream),
                        _ => unreachable!(),
                    };

                    let msg = KadResponseMsg::FindNode {
                        closer_peers: closer_peers.clone(),
                    };
                    self.substreams
                        .push(SubstreamState::InPendingSend(conn_id, substream, msg));
                }
            }
            KademliaHandlerIn::GetProvidersReq { key, user_data } => {
                let msg = KadRequestMsg::GetProviders { key: key.clone() };
                self.substreams
                    .push(SubstreamState::OutPendingOpen(msg, Some(user_data.clone())));
            }
            KademliaHandlerIn::GetProvidersRes {
                closer_peers,
                provider_peers,
                request_id,
            } => {
                let pos = self.substreams.iter().position(|state| match state {
                    SubstreamState::InWaitingUser(ref conn_id, _)
                        if conn_id == &request_id.connec_unique_id =>
                    {
                        true
                    }
                    _ => false,
                });

                if let Some(pos) = pos {
                    let (conn_id, substream) = match self.substreams.remove(pos) {
                        SubstreamState::InWaitingUser(conn_id, substream) => (conn_id, substream),
                        _ => unreachable!(),
                    };

                    let msg = KadResponseMsg::GetProviders {
                        closer_peers: closer_peers.clone(),
                        provider_peers: provider_peers.clone(),
                    };
                    self.substreams
                        .push(SubstreamState::InPendingSend(conn_id, substream, msg));
                }
            }
            KademliaHandlerIn::AddProvider { key, provider_peer } => {
                let msg = KadRequestMsg::AddProvider {
                    key: key.clone(),
                    provider_peer: provider_peer.clone(),
                };
                self.substreams
                    .push(SubstreamState::OutPendingOpen(msg, None));
            }
            KademliaHandlerIn::GetValue { key, user_data } => {
                let msg = KadRequestMsg::GetValue { key };
                self.substreams
                    .push(SubstreamState::OutPendingOpen(msg, Some(user_data)));

            }
            KademliaHandlerIn::PutValue { key, value, user_data } => {
                let msg = KadRequestMsg::PutValue {
                    key,
                    value,
                };

                self.substreams
                    .push(SubstreamState::OutPendingOpen(msg, Some(user_data)));
            }
            KademliaHandlerIn::GetValueRes {
                result,
                closer_peers,
                request_id,
            } => {
                let pos = self.substreams.iter().position(|state| match state {
                    SubstreamState::InWaitingUser(ref conn_id, _)
                        => conn_id == &request_id.connec_unique_id,
                    _ => false,
                });

                if let Some(pos) = pos {
                    let (conn_id, substream) = match self.substreams.remove(pos) {
                        SubstreamState::InWaitingUser(conn_id, substream) => (conn_id, substream),
                        _ => unreachable!(),
                    };

                    let msg = KadResponseMsg::GetValue {
                        result,
                        closer_peers: closer_peers.clone(),
                    };
                    self.substreams
                        .push(SubstreamState::InPendingSend(conn_id, substream, msg));
                }
            }
            KademliaHandlerIn::PutValueRes {
                key,
                request_id,
                value,
            } => {
                let pos = self.substreams.iter().position(|state| match state {
                    SubstreamState::InWaitingUser(ref conn_id, _)
                        if conn_id == &request_id.connec_unique_id =>
                        {
                            true
                        }
                    _ => false,
                });

                if let Some(pos) = pos {
                    let (conn_id, substream) = match self.substreams.remove(pos) {
                        SubstreamState::InWaitingUser(conn_id, substream) => (conn_id, substream),
                        _ => unreachable!(),
                    };

                    let msg = KadResponseMsg::PutValue {
                        key,
                        value,
                    };
                    self.substreams
                        .push(SubstreamState::InPendingSend(conn_id, substream, msg));
                }
            }
        }
    }

    #[inline]
    fn inject_dial_upgrade_error(
        &mut self,
        (_, user_data): Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<io::Error>,
    ) {
        // TODO: cache the fact that the remote doesn't support kademlia at all, so that we don't
        //       continue trying
        if let Some(user_data) = user_data {
            self.substreams
                .push(SubstreamState::OutReportError(error.into(), user_data));
        }
    }

    #[inline]
    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        io::Error,
    > {
        // We remove each element from `substreams` one by one and add them back.
        for n in (0..self.substreams.len()).rev() {
            let mut substream = self.substreams.swap_remove(n);

            loop {
                match advance_substream(substream, self.config.clone()) {
                    (Some(new_state), Some(event), _) => {
                        self.substreams.push(new_state);
                        return Ok(Async::Ready(event));
                    }
                    (None, Some(event), _) => {
                        return Ok(Async::Ready(event));
                    }
                    (Some(new_state), None, false) => {
                        self.substreams.push(new_state);
                        break;
                    }
                    (Some(new_state), None, true) => {
                        substream = new_state;
                        continue;
                    }
                    (None, None, _) => {
                        break;
                    }
                }
            }
        }

        if self.substreams.is_empty() {
            self.keep_alive = KeepAlive::Until(Instant::now() + Duration::from_secs(10));
        } else {
            self.keep_alive = KeepAlive::Yes;
        }

        Ok(Async::NotReady)
    }
}

/// Advances one substream.
///
/// Returns the new state for that substream, an event to generate, and whether the substream
/// should be polled again.
fn advance_substream<TSubstream, TUserData>(
    state: SubstreamState<TSubstream, TUserData>,
    upgrade: KademliaProtocolConfig,
) -> (
    Option<SubstreamState<TSubstream, TUserData>>,
    Option<
        ProtocolsHandlerEvent<
            KademliaProtocolConfig,
            (KadRequestMsg, Option<TUserData>),
            KademliaHandlerEvent<TUserData>,
        >,
    >,
    bool,
)
where
    TSubstream: AsyncRead + AsyncWrite,
{
    match state {
        SubstreamState::OutPendingOpen(msg, user_data) => {
            let ev = ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(upgrade),
                info: (msg, user_data),
            };
            (None, Some(ev), false)
        }
        SubstreamState::OutPendingSend(mut substream, msg, user_data) => {
            match substream.start_send(msg) {
                Ok(AsyncSink::Ready) => (
                    Some(SubstreamState::OutPendingFlush(substream, user_data)),
                    None,
                    true,
                ),
                Ok(AsyncSink::NotReady(msg)) => (
                    Some(SubstreamState::OutPendingSend(substream, msg, user_data)),
                    None,
                    false,
                ),
                Err(error) => {
                    let event = if let Some(user_data) = user_data {
                        Some(ProtocolsHandlerEvent::Custom(KademliaHandlerEvent::QueryError {
                            error: KademliaHandlerQueryErr::Io(error),
                            user_data
                        }))
                    } else {
                        None
                    };

                    (None, event, false)
                }
            }
        }
        SubstreamState::OutPendingFlush(mut substream, user_data) => {
            match substream.poll_complete() {
                Ok(Async::Ready(())) => {
                    if let Some(user_data) = user_data {
                        (
                            Some(SubstreamState::OutWaitingAnswer(substream, user_data)),
                            None,
                            true,
                        )
                    } else {
                        (Some(SubstreamState::OutClosing(substream)), None, true)
                    }
                }
                Ok(Async::NotReady) => (
                    Some(SubstreamState::OutPendingFlush(substream, user_data)),
                    None,
                    false,
                ),
                Err(error) => {
                    let event = if let Some(user_data) = user_data {
                        Some(ProtocolsHandlerEvent::Custom(KademliaHandlerEvent::QueryError {
                            error: KademliaHandlerQueryErr::Io(error),
                            user_data,
                        }))
                    } else {
                        None
                    };

                    (None, event, false)
                }
            }
        }
        SubstreamState::OutWaitingAnswer(mut substream, user_data) => match substream.poll() {
            Ok(Async::Ready(Some(msg))) => {
                let new_state = SubstreamState::OutClosing(substream);
                let event = process_kad_response(msg, user_data);
                (
                    Some(new_state),
                    Some(ProtocolsHandlerEvent::Custom(event)),
                    true,
                )
            }
            Ok(Async::NotReady) => (
                Some(SubstreamState::OutWaitingAnswer(substream, user_data)),
                None,
                false,
            ),
            Err(error) => {
                let event = KademliaHandlerEvent::QueryError {
                    error: KademliaHandlerQueryErr::Io(error),
                    user_data,
                };
                (None, Some(ProtocolsHandlerEvent::Custom(event)), false)
            }
            Ok(Async::Ready(None)) => {
                let event = KademliaHandlerEvent::QueryError {
                    error: KademliaHandlerQueryErr::Io(io::ErrorKind::UnexpectedEof.into()),
                    user_data,
                };
                (None, Some(ProtocolsHandlerEvent::Custom(event)), false)
            }
        },
        SubstreamState::OutReportError(error, user_data) => {
            let event = KademliaHandlerEvent::QueryError { error, user_data };
            (None, Some(ProtocolsHandlerEvent::Custom(event)), false)
        }
        SubstreamState::OutClosing(mut stream) => match stream.close() {
            Ok(Async::Ready(())) => (None, None, false),
            Ok(Async::NotReady) => (Some(SubstreamState::OutClosing(stream)), None, false),
            Err(_) => (None, None, false),
        },
        SubstreamState::InWaitingMessage(id, mut substream) => match substream.poll() {
            Ok(Async::Ready(Some(msg))) => {
                if let Ok(ev) = process_kad_request(msg, id) {
                    (
                        Some(SubstreamState::InWaitingUser(id, substream)),
                        Some(ProtocolsHandlerEvent::Custom(ev)),
                        false,
                    )
                } else {
                    (Some(SubstreamState::InClosing(substream)), None, true)
                }
            }
            Ok(Async::NotReady) => (
                Some(SubstreamState::InWaitingMessage(id, substream)),
                None,
                false,
            ),
            Ok(Async::Ready(None)) | Err(_) => (None, None, false),
        },
        SubstreamState::InWaitingUser(id, substream) => (
            Some(SubstreamState::InWaitingUser(id, substream)),
            None,
            false,
        ),
        SubstreamState::InPendingSend(id, mut substream, msg) => match substream.start_send(msg) {
            Ok(AsyncSink::Ready) => (
                Some(SubstreamState::InPendingFlush(id, substream)),
                None,
                true,
            ),
            Ok(AsyncSink::NotReady(msg)) => (
                Some(SubstreamState::InPendingSend(id, substream, msg)),
                None,
                false,
            ),
            Err(_) => (None, None, false),
        },
        SubstreamState::InPendingFlush(id, mut substream) => match substream.poll_complete() {
            Ok(Async::Ready(())) => (
                Some(SubstreamState::InWaitingMessage(id, substream)),
                None,
                true,
            ),
            Ok(Async::NotReady) => (
                Some(SubstreamState::InPendingFlush(id, substream)),
                None,
                false,
            ),
            Err(_) => (None, None, false),
        },
        SubstreamState::InClosing(mut stream) => match stream.close() {
            Ok(Async::Ready(())) => (None, None, false),
            Ok(Async::NotReady) => (Some(SubstreamState::InClosing(stream)), None, false),
            Err(_) => (None, None, false),
        },
    }
}

/// Processes a Kademlia message that's expected to be a request from a remote.
fn process_kad_request<TUserData>(
    event: KadRequestMsg,
    connec_unique_id: UniqueConnecId,
) -> Result<KademliaHandlerEvent<TUserData>, io::Error> {
    match event {
        KadRequestMsg::Ping => {
            // TODO: implement; although in practice the PING message is never
            //       used, so we may consider removing it altogether
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the PING Kademlia message is not implemented",
            ))
        }
        KadRequestMsg::FindNode { key } => Ok(KademliaHandlerEvent::FindNodeReq {
            key,
            request_id: KademliaRequestId { connec_unique_id },
        }),
        KadRequestMsg::GetProviders { key } => Ok(KademliaHandlerEvent::GetProvidersReq {
            key,
            request_id: KademliaRequestId { connec_unique_id },
        }),
        KadRequestMsg::AddProvider { key, provider_peer } => {
            Ok(KademliaHandlerEvent::AddProvider { key, provider_peer })
        }
        KadRequestMsg::GetValue { key } => Ok(KademliaHandlerEvent::GetValue {
            key,
            request_id: KademliaRequestId { connec_unique_id },
        }),
        KadRequestMsg::PutValue { key, value } => Ok(KademliaHandlerEvent::PutValue {
            key,
            value,
            request_id: KademliaRequestId { connec_unique_id },
        })
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
        KadResponseMsg::FindNode { closer_peers } => {
            KademliaHandlerEvent::FindNodeRes {
                closer_peers,
                user_data,
            }
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
            result,
            closer_peers,
        } => KademliaHandlerEvent::GetValueRes {
            result,
            closer_peers,
            user_data,
        },
        KadResponseMsg::PutValue { key, .. } => {
            KademliaHandlerEvent::PutValueRes {
                key,
                user_data,
            }
        }
    }
}
