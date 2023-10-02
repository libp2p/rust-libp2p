// Copyright 2023 Protocol Labs.
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

use instant::Duration;

use std::{
    collections::HashSet,
    task::{Context, Poll},
};

use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_request_response as request_response;
use libp2p_swarm::{
    derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionId, FromSwarm,
    NetworkBehaviour, PollParameters, THandlerInEvent, THandlerOutEvent, ToSwarm,
};

use crate::{protocol::Response, RunDuration, RunParams};

/// Connection identifier.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct RunId(request_response::RequestId);

impl From<request_response::RequestId> for RunId {
    fn from(value: request_response::RequestId) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub struct Event {
    pub id: RunId,
    pub result: Result<RunDuration, request_response::OutboundFailure>,
}

pub struct Behaviour {
    connected: HashSet<PeerId>,
    request_response: request_response::Behaviour<crate::protocol::Codec>,
}

impl Default for Behaviour {
    fn default() -> Self {
        let mut req_resp_config = request_response::Config::default();
        req_resp_config.set_connection_keep_alive(Duration::from_secs(60 * 5));
        req_resp_config.set_request_timeout(Duration::from_secs(60 * 5));
        Self {
            connected: Default::default(),
            request_response: request_response::Behaviour::new(
                std::iter::once((
                    crate::PROTOCOL_NAME,
                    request_response::ProtocolSupport::Outbound,
                )),
                req_resp_config,
            ),
        }
    }
}

impl Behaviour {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn perf(&mut self, server: PeerId, params: RunParams) -> Result<RunId, PerfError> {
        if !self.connected.contains(&server) {
            return Err(PerfError::NotConnected);
        }

        let id = self.request_response.send_request(&server, params).into();

        Ok(id)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum PerfError {
    #[error("Not connected to peer")]
    NotConnected,
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        <request_response::Behaviour<crate::protocol::Codec> as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: libp2p_core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p_swarm::ConnectionDenied> {
        self.request_response.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: libp2p_core::Endpoint,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        self.request_response
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), libp2p_swarm::ConnectionDenied> {
        self.request_response.handle_pending_inbound_connection(
            connection_id,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        self.request_response.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished { peer_id, .. }) => {
                self.connected.insert(peer_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id: _,
                endpoint: _,
                handler: _,
                remaining_established,
            }) => {
                if remaining_established == 0 {
                    assert!(self.connected.remove(&peer_id));
                }
            }
            _ => {}
        };

        self.request_response.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.request_response
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.request_response.poll(cx, params).map(|to_swarm| {
            to_swarm.map_out(|m| match m {
                request_response::Event::Message {
                    peer: _,
                    message:
                        request_response::Message::Response {
                            request_id,
                            response: Response::Receiver(run_duration),
                        },
                } => Event {
                    id: request_id.into(),
                    result: Ok(run_duration),
                },
                request_response::Event::Message {
                    peer: _,
                    message:
                        request_response::Message::Response {
                            response: Response::Sender(_),
                            ..
                        },
                } => unreachable!(),
                request_response::Event::Message {
                    peer: _,
                    message: request_response::Message::Request { .. },
                } => {
                    unreachable!()
                }
                request_response::Event::OutboundFailure {
                    peer: _,
                    request_id,
                    error,
                } => Event {
                    id: request_id.into(),
                    result: Err(error),
                },
                request_response::Event::InboundFailure {
                    peer: _,
                    request_id: _,
                    error: _,
                } => unreachable!(),
                request_response::Event::ResponseSent { .. } => unreachable!(),
            })
        })
    }
}
