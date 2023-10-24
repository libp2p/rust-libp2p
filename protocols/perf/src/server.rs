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

use std::task::{Context, Poll};

use instant::Duration;
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_request_response as request_response;
use libp2p_swarm::{
    ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent, THandlerOutEvent, ToSwarm,
};

use crate::protocol::Response;

pub struct Behaviour {
    request_response: request_response::Behaviour<crate::protocol::Codec>,
}

impl Default for Behaviour {
    fn default() -> Self {
        let mut req_resp_config = request_response::Config::default();
        req_resp_config.set_request_timeout(Duration::from_secs(60 * 5));

        Self {
            request_response: request_response::Behaviour::new(
                std::iter::once((
                    crate::PROTOCOL_NAME,
                    request_response::ProtocolSupport::Inbound,
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
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        <request_response::Behaviour<crate::protocol::Codec> as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = ();

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

    fn on_swarm_event(&mut self, event: FromSwarm) {
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
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.request_response.poll(cx).map(|to_swarm| {
            to_swarm.map_out(|m| match m {
                request_response::Event::Message {
                    peer: _,
                    message: request_response::Message::Response { .. },
                } => {
                    unreachable!()
                }
                request_response::Event::Message {
                    peer: _,
                    message:
                        request_response::Message::Request {
                            request_id: _,
                            request,
                            channel,
                        },
                } => {
                    let _ = self
                        .request_response
                        .send_response(channel, Response::Sender(request.to_send));
                }
                request_response::Event::OutboundFailure { .. } => unreachable!(),
                request_response::Event::InboundFailure { .. } => {}
                request_response::Event::ResponseSent { .. } => {}
            })
        })
    }
}
