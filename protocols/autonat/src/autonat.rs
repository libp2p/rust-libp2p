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

pub use crate::protocol::DialResponse;
use crate::protocol::{AutoNatCodec, AutoNatProtocol};
use libp2p_core::{
    connection::{ConnectionId, ListenerId},
    ConnectedPoint, Multiaddr, PeerId,
};
use libp2p_request_response::{
    handler::RequestResponseHandlerEvent, ProtocolSupport, RequestResponse, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p_swarm::{
    IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
};
use std::{
    iter,
    task::{Context, Poll},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutoNatEvent {
    DialResponse(DialResponse),
}

pub struct AutoNat {
    inner: RequestResponse<AutoNatCodec>,
}

impl AutoNat {
    pub fn new() -> Self {
        let protocols = iter::once((AutoNatProtocol, ProtocolSupport::Full));
        let cfg = RequestResponseConfig::default();
        let inner = RequestResponse::new(AutoNatCodec, protocols, cfg);
        Self { inner }
    }

    pub fn add_address(&mut self, peer_id: &PeerId, addr: Multiaddr) {
        self.inner.add_address(peer_id, addr)
    }
}

impl NetworkBehaviour for AutoNat {
    type ProtocolsHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = AutoNatEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.inner.addresses_of_peer(peer)
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        self.inner.inject_connected(peer)
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        self.inner.inject_disconnected(peer)
    }

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.inner
            .inject_connection_established(peer, conn, endpoint)
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
        handler: <Self::ProtocolsHandler as IntoProtocolsHandler>::Handler,
    ) {
        self.inner
            .inject_connection_closed(peer, conn, endpoint, handler)
    }

    fn inject_address_change(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        self.inner.inject_address_change(peer, conn, old, new)
    }

    fn inject_event(
        &mut self,
        peer: PeerId,
        conn: ConnectionId,
        event: RequestResponseHandlerEvent<AutoNatCodec>,
    ) {
        self.inner.inject_event(peer, conn, event)
    }

    fn inject_addr_reach_failure(
        &mut self,
        peer_id: Option<&PeerId>,
        addr: &Multiaddr,
        error: &dyn std::error::Error,
    ) {
        self.inner.inject_addr_reach_failure(peer_id, addr, error)
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: &PeerId,
        handler: Self::ProtocolsHandler,
        error: libp2p_swarm::DialError,
    ) {
        self.inner.inject_dial_failure(peer_id, handler, error)
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &Multiaddr,
        send_back_addr: &Multiaddr,
        handler: Self::ProtocolsHandler,
    ) {
        self.inner
            .inject_listen_failure(local_addr, send_back_addr, handler)
    }

    fn inject_new_listener(&mut self, id: ListenerId) {
        self.inner.inject_new_listener(id)
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_new_listen_addr(id, addr)
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        self.inner.inject_expired_listen_addr(id, addr)
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        self.inner.inject_listener_error(id, err)
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        self.inner.inject_listener_closed(id, reason)
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_new_external_addr(addr)
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        self.inner.inject_expired_external_addr(addr)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        loop {
            match self.inner.poll(cx, params) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::Message { peer, message },
                )) => match message {
                    RequestResponseMessage::Request {
                        request_id,
                        request,
                        channel,
                    } => {
                        println!("{} {:?} {:?} {:?}", peer, request_id, request, channel);
                    }
                    RequestResponseMessage::Response {
                        request_id,
                        response,
                    } => {
                        println!("{} {:?} {:?}", peer, request_id, response);
                    }
                },
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::ResponseSent { peer, request_id },
                )) => {
                    println!("response sent {} {:?}", peer, request_id);
                }
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::OutboundFailure {
                        peer,
                        request_id,
                        error,
                    },
                )) => {
                    println!("outbound failure {} {:?} {:?}", peer, request_id, error);
                }
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::InboundFailure {
                        peer,
                        error,
                        request_id,
                    },
                )) => {
                    println!("inbound failure {} {:?} {:?}", peer, request_id, error);
                }
                Poll::Ready(NetworkBehaviourAction::DialAddress { address, handler }) => {
                    return Poll::Ready(NetworkBehaviourAction::DialAddress { address, handler })
                }
                Poll::Ready(NetworkBehaviourAction::DialPeer {
                    peer_id,
                    condition,
                    handler,
                }) => {
                    return Poll::Ready(NetworkBehaviourAction::DialPeer {
                        peer_id,
                        condition,
                        handler,
                    })
                }
                Poll::Ready(NetworkBehaviourAction::CloseConnection {
                    peer_id,
                    connection,
                }) => {
                    return Poll::Ready(NetworkBehaviourAction::CloseConnection {
                        peer_id,
                        connection,
                    })
                }
                Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                    peer_id,
                    handler,
                    event,
                }) => {
                    return Poll::Ready(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler,
                        event,
                    })
                }
                Poll::Ready(NetworkBehaviourAction::ReportObservedAddr { address, score }) => {
                    return Poll::Ready(NetworkBehaviourAction::ReportObservedAddr {
                        address,
                        score,
                    })
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
