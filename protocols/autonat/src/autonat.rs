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

use crate::protocol::{AutoNatCodec, AutoNatProtocol};
pub use crate::protocol::{DialRequest, DialResponse, ResponseStatus};
use futures::future::FutureExt;
use libp2p_core::{connection::ConnectionId, ConnectedPoint, Multiaddr, PeerId};
use libp2p_request_response::{
    handler::RequestProtocol, handler::RequestResponseHandlerEvent, ProtocolSupport,
    RequestResponse, RequestResponseConfig, RequestResponseEvent, RequestResponseMessage,
    ResponseChannel,
};
use libp2p_swarm::{DialPeerCondition, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use std::{
    collections::HashMap,
    iter,
    task::{Context, Poll},
    time::Duration,
};
use wasm_timer::Delay;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoNatConfig {
    timeout: Duration,
}

impl Default for AutoNatConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutoNatEvent {
    DialResponse(DialResponse),
}

pub struct AutoNat {
    config: AutoNatConfig,
    inner: RequestResponse<AutoNatCodec>,
    observed_addresses: HashMap<PeerId, HashMap<ConnectionId, Multiaddr>>,
    requests: HashMap<PeerId, (ResponseChannel<DialResponse>, Delay)>,
}

impl Default for AutoNat {
    fn default() -> Self {
        Self::new(AutoNatConfig::default())
    }
}

impl AutoNat {
    pub fn new(config: AutoNatConfig) -> Self {
        let protocols = iter::once((AutoNatProtocol, ProtocolSupport::Full));
        let cfg = RequestResponseConfig::default();
        let inner = RequestResponse::new(AutoNatCodec, protocols, cfg);
        Self {
            config,
            inner,
            observed_addresses: Default::default(),
            requests: Default::default(),
        }
    }

    pub fn add_address(&mut self, peer: &PeerId, addr: Multiaddr) {
        self.inner.add_address(peer, addr)
    }

    pub fn dial_request(&mut self, peer: &PeerId) {
        self.inner.send_request(peer, DialRequest::default());
    }
}

impl NetworkBehaviour for AutoNat {
    type ProtocolsHandler = <RequestResponse<AutoNatCodec> as NetworkBehaviour>::ProtocolsHandler;
    type OutEvent = AutoNatEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        let mut addresses = self.inner.addresses_of_peer(peer);
        if let Some(conns) = self.observed_addresses.get(peer) {
            for addr in conns.values() {
                addresses.push(addr.clone());
            }
        }
        addresses
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        self.inner.inject_connected(peer)
    }

    fn inject_connection_established(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.inner
            .inject_connection_established(peer, conn, endpoint);

        let addr = match endpoint {
            ConnectedPoint::Dialer { address, .. } => address.clone(),
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr.clone(),
        };
        self.observed_addresses
            .entry(peer.clone())
            .or_default()
            .insert(*conn, addr);

        if let ConnectedPoint::Dialer { address, .. } = endpoint {
            if let Some((channel, _)) = self.requests.remove(peer) {
                self.inner
                    .send_response(channel, DialResponse::Ok(address.clone()));
            }
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        conn: &ConnectionId,
        endpoint: &ConnectedPoint,
    ) {
        self.inner.inject_connection_closed(peer, conn, endpoint);
        if let Some(addrs) = self.observed_addresses.get_mut(peer) {
            addrs.remove(conn);
        }
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        self.inner.inject_disconnected(peer);
        self.observed_addresses.remove(peer);
    }

    fn inject_dial_failure(&mut self, peer: &PeerId) {
        self.inner.inject_dial_failure(peer);
    }

    fn inject_event(
        &mut self,
        peer: PeerId,
        conn: ConnectionId,
        event: RequestResponseHandlerEvent<AutoNatCodec>,
    ) {
        self.inner.inject_event(peer, conn, event)
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        pp: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<AutoNatCodec>, AutoNatEvent>> {
        loop {
            match self.inner.poll(cx, pp) {
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::Message { peer, message },
                )) => match message {
                    RequestResponseMessage::Request {
                        request: _,
                        channel,
                    } => {
                        let timeout = Delay::new(self.config.timeout);
                        self.requests.insert(peer.clone(), (channel, timeout));
                        return Poll::Ready(NetworkBehaviourAction::DialPeer {
                            peer_id: peer,
                            condition: DialPeerCondition::NotDialing,
                        });
                    }
                    RequestResponseMessage::Response {
                        request_id: _,
                        response,
                    } => {
                        let event = AutoNatEvent::DialResponse(response);
                        return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    }
                },
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::OutboundFailure {
                        peer,
                        request_id,
                        error,
                    },
                )) => {
                    log::error!(
                        "autonat outbound failure {} {:?} {:?}",
                        peer,
                        request_id,
                        error
                    );
                }
                Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    RequestResponseEvent::InboundFailure { peer, error },
                )) => {
                    log::error!("autonat inbound failure {} {:?}", peer, error);
                }
                Poll::Ready(NetworkBehaviourAction::DialAddress { address }) => {
                    return Poll::Ready(NetworkBehaviourAction::DialAddress { address })
                }
                Poll::Ready(NetworkBehaviourAction::DialPeer { peer_id, condition }) => {
                    return Poll::Ready(NetworkBehaviourAction::DialPeer { peer_id, condition })
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
                Poll::Ready(NetworkBehaviourAction::ReportObservedAddr { address }) => {
                    return Poll::Ready(NetworkBehaviourAction::ReportObservedAddr { address })
                }
                Poll::Pending => break,
            }
        }
        self.requests = std::mem::replace(&mut self.requests, Default::default())
            .into_iter()
            .filter_map(|(peer, (channel, mut timeout))| {
                if let Poll::Pending = timeout.poll_unpin(cx) {
                    Some((peer, (channel, timeout)))
                } else {
                    let response = DialResponse::Err(ResponseStatus::EDialError, "".into());
                    self.inner.send_response(channel, response);
                    None
                }
            })
            .collect();
        Poll::Pending
    }
}
