// Copyright 2021 Protocol Labs.
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

use std::time::Duration;

use asynchronous_codec::{Framed, FramedParts};
use either::Either;
use futures::channel::oneshot::{self};
use futures::prelude::*;
use thiserror::Error;

use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionHandlerEvent, ConnectionId, Stream, StreamUpgradeError};

use crate::behaviour::handler;
use crate::behaviour::handler::Config;
use crate::protocol::{inbound_hop, MAX_MESSAGE_SIZE};
use crate::{proto, CircuitId};

#[derive(Debug, Error)]
pub(crate) enum UpgradeError {
    #[error("Circuit failed")]
    CircuitFailed(#[from] CircuitFailedReason),
    #[error("Fatal")]
    Fatal(#[from] FatalUpgradeError),
}

impl From<quick_protobuf_codec::Error> for UpgradeError {
    fn from(error: quick_protobuf_codec::Error) -> Self {
        Self::Fatal(error.into())
    }
}

#[derive(Debug, Error)]
pub enum CircuitFailedReason {
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Remote reported permission denied.")]
    PermissionDenied,
}

#[derive(Debug, Error)]
pub enum FatalUpgradeError {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("Stream closed")]
    StreamClosed,
    #[error("Expected 'status' field to be set.")]
    MissingStatusField,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Failed to parse response type field.")]
    ParseStatusField,
    #[error("Unexpected message status '{0:?}'")]
    UnexpectedStatus(proto::Status),
}

/// Attempts to _connect_ to a peer via the given stream.
pub(crate) async fn connect(
    io: Stream,
    stop_command: PendingConnect,
    tx: oneshot::Sender<()>,
) -> handler::RelayConnectionHandlerEvent {
    let msg = proto::StopMessage {
        type_pb: proto::StopMessageType::CONNECT,
        peer: Some(proto::Peer {
            id: stop_command.src_peer_id.to_bytes(),
            addrs: vec![],
        }),
        limit: Some(proto::Limit {
            duration: Some(
                stop_command
                    .max_circuit_duration
                    .as_secs()
                    .try_into()
                    .expect("`max_circuit_duration` not to exceed `u32::MAX`."),
            ),
            data: Some(stop_command.max_circuit_bytes),
        }),
        status: None,
    };

    let mut substream = Framed::new(io, quick_protobuf_codec::Codec::new(MAX_MESSAGE_SIZE));

    if substream.send(msg).await.is_err() {
        return ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(Either::Right(
            FatalUpgradeError::StreamClosed,
        )));
    }

    let res = substream.next().await;

    if let None | Some(Err(_)) = res {
        return ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(Either::Right(
            FatalUpgradeError::StreamClosed,
        )));
    }

    let proto::StopMessage {
        type_pb,
        peer: _,
        limit: _,
        status,
    } = res.unwrap().expect("should be ok");

    match type_pb {
        proto::StopMessageType::CONNECT => {
            return ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(Either::Right(
                FatalUpgradeError::UnexpectedTypeConnect,
            )))
        }
        proto::StopMessageType::STATUS => {}
    }

    match status {
        Some(proto::Status::OK) => {}
        Some(proto::Status::RESOURCE_LIMIT_EXCEEDED) => {
            return ConnectionHandlerEvent::NotifyBehaviour(
                handler::Event::OutboundConnectNegotiationFailed {
                    circuit_id: stop_command.circuit_id,
                    src_peer_id: stop_command.src_peer_id,
                    src_connection_id: stop_command.src_connection_id,
                    inbound_circuit_req: stop_command.inbound_circuit_req,
                    status: proto::Status::RESOURCE_LIMIT_EXCEEDED,
                    error: StreamUpgradeError::Apply(CircuitFailedReason::ResourceLimitExceeded),
                },
            )
        }
        Some(proto::Status::PERMISSION_DENIED) => {
            return ConnectionHandlerEvent::NotifyBehaviour(
                handler::Event::OutboundConnectNegotiationFailed {
                    circuit_id: stop_command.circuit_id,
                    src_peer_id: stop_command.src_peer_id,
                    src_connection_id: stop_command.src_connection_id,
                    inbound_circuit_req: stop_command.inbound_circuit_req,
                    status: proto::Status::PERMISSION_DENIED,
                    error: StreamUpgradeError::Apply(CircuitFailedReason::PermissionDenied),
                },
            )
        }
        Some(s) => {
            return ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(Either::Right(
                FatalUpgradeError::UnexpectedStatus(s),
            )))
        }
        None => {
            return ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(Either::Right(
                FatalUpgradeError::MissingStatusField,
            )))
        }
    }

    let FramedParts {
        io,
        read_buffer,
        write_buffer,
        ..
    } = substream.into_parts();
    assert!(
        write_buffer.is_empty(),
        "Expect a flushed Framed to have an empty write buffer."
    );

    ConnectionHandlerEvent::NotifyBehaviour(handler::Event::OutboundConnectNegotiated {
        circuit_id: stop_command.circuit_id,
        src_peer_id: stop_command.src_peer_id,
        src_connection_id: stop_command.src_connection_id,
        inbound_circuit_req: stop_command.inbound_circuit_req,
        dst_handler_notifier: tx,
        dst_stream: io,
        dst_pending_data: read_buffer.freeze(),
    })
}

pub(crate) struct PendingConnect {
    pub(crate) circuit_id: CircuitId,
    pub(crate) inbound_circuit_req: inbound_hop::CircuitReq,
    pub(crate) src_peer_id: PeerId,
    pub(crate) src_connection_id: ConnectionId,
    max_circuit_duration: Duration,
    max_circuit_bytes: u64,
}

impl PendingConnect {
    pub(crate) fn new(
        circuit_id: CircuitId,
        inbound_circuit_req: inbound_hop::CircuitReq,
        src_peer_id: PeerId,
        src_connection_id: ConnectionId,
        config: &Config,
    ) -> Self {
        Self {
            circuit_id,
            inbound_circuit_req,
            src_peer_id,
            src_connection_id,
            max_circuit_duration: config.max_circuit_duration,
            max_circuit_bytes: config.max_circuit_bytes,
        }
    }
}
