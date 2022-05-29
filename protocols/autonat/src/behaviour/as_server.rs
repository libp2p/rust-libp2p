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

use super::{
    Action, AutoNatCodec, Config, DialRequest, DialResponse, Event, HandleInnerEvent, ProbeId,
    ResponseError,
};
use instant::Instant;
use libp2p_core::{connection::ConnectionId, multiaddr::Protocol, Multiaddr, PeerId};
use libp2p_request_response::{
    InboundFailure, RequestId, RequestResponse, RequestResponseEvent, RequestResponseMessage,
    ResponseChannel,
};
use libp2p_swarm::{
    dial_opts::{DialOpts, PeerCondition},
    DialError, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    num::NonZeroU8,
};

/// Inbound probe failed.
#[derive(Debug, Clone, PartialEq)]
pub enum InboundProbeError {
    /// Receiving the dial-back request or sending a response failed.
    InboundRequest(InboundFailure),
    /// We refused or failed to dial the client.
    Response(ResponseError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InboundProbeEvent {
    /// A dial-back request was received from a remote peer.
    Request {
        probe_id: ProbeId,
        /// Peer that sent the request.
        peer: PeerId,
        /// The addresses that will be attempted to dial.
        addresses: Vec<Multiaddr>,
    },
    /// A dial request to the remote was successful.
    Response {
        probe_id: ProbeId,
        /// Peer to which the response is sent.
        peer: PeerId,
        address: Multiaddr,
    },
    /// The inbound request failed, was rejected, or none of the remote's
    /// addresses could be dialed.
    Error {
        probe_id: ProbeId,
        /// Peer that sent the dial-back request.
        peer: PeerId,
        error: InboundProbeError,
    },
}

/// View over [`super::Behaviour`] in a server role.
pub struct AsServer<'a> {
    pub inner: &'a mut RequestResponse<AutoNatCodec>,
    pub config: &'a Config,
    pub connected: &'a HashMap<PeerId, HashMap<ConnectionId, Option<Multiaddr>>>,
    pub probe_id: &'a mut ProbeId,

    pub throttled_clients: &'a mut Vec<(PeerId, Instant)>,

    #[allow(clippy::type_complexity)]
    pub ongoing_inbound: &'a mut HashMap<
        PeerId,
        (
            ProbeId,
            RequestId,
            Vec<Multiaddr>,
            ResponseChannel<DialResponse>,
        ),
    >,
}

impl<'a> HandleInnerEvent for AsServer<'a> {
    fn handle_event(
        &mut self,
        _params: &mut impl PollParameters,
        event: RequestResponseEvent<DialRequest, DialResponse>,
    ) -> (VecDeque<Event>, Option<Action>) {
        let mut events = VecDeque::new();
        let mut action = None;
        match event {
            RequestResponseEvent::Message {
                peer,
                message:
                    RequestResponseMessage::Request {
                        request_id,
                        request,
                        channel,
                    },
            } => {
                let probe_id = self.probe_id.next();
                match self.resolve_inbound_request(peer, request) {
                    Ok(addrs) => {
                        log::debug!(
                            "Inbound dial request from Peer {} with dial-back addresses {:?}.",
                            peer,
                            addrs
                        );

                        self.ongoing_inbound
                            .insert(peer, (probe_id, request_id, addrs.clone(), channel));
                        self.throttled_clients.push((peer, Instant::now()));

                        events.push_back(Event::InboundProbe(InboundProbeEvent::Request {
                            probe_id,
                            peer,
                            addresses: addrs.clone(),
                        }));

                        action = Some(NetworkBehaviourAction::Dial {
                            opts: DialOpts::peer_id(peer)
                                .condition(PeerCondition::Always)
                                .override_dial_concurrency_factor(NonZeroU8::new(1).expect("1 > 0"))
                                .addresses(addrs)
                                .build(),
                            handler: self.inner.new_handler(),
                        });
                    }
                    Err((status_text, error)) => {
                        log::debug!(
                            "Reject inbound dial request from peer {}: {}.",
                            peer,
                            status_text
                        );

                        let response = DialResponse {
                            result: Err(error.clone()),
                            status_text: Some(status_text),
                        };
                        let _ = self.inner.send_response(channel, response);

                        events.push_back(Event::InboundProbe(InboundProbeEvent::Error {
                            probe_id,
                            peer,
                            error: InboundProbeError::Response(error),
                        }));
                    }
                }
            }
            RequestResponseEvent::InboundFailure {
                peer,
                error,
                request_id,
            } => {
                log::debug!(
                    "Inbound Failure {} when on dial-back request from peer {}.",
                    error,
                    peer
                );

                let probe_id = match self.ongoing_inbound.get(&peer) {
                    Some((_, rq_id, _, _)) if *rq_id == request_id => {
                        self.ongoing_inbound.remove(&peer).unwrap().0
                    }
                    _ => self.probe_id.next(),
                };

                events.push_back(Event::InboundProbe(InboundProbeEvent::Error {
                    probe_id,
                    peer,
                    error: InboundProbeError::InboundRequest(error),
                }));
            }
            _ => {}
        }
        (events, action)
    }
}

impl<'a> AsServer<'a> {
    pub fn on_outbound_connection(
        &mut self,
        peer: &PeerId,
        address: &Multiaddr,
    ) -> Option<InboundProbeEvent> {
        let (_, _, addrs, _) = self.ongoing_inbound.get(peer)?;

        // Check if the dialed address was among the requested addresses.
        if !addrs.contains(address) {
            return None;
        }

        log::debug!(
            "Dial-back to peer {} succeeded at addr {:?}.",
            peer,
            address
        );

        let (probe_id, _, _, channel) = self.ongoing_inbound.remove(peer).unwrap();
        let response = DialResponse {
            result: Ok(address.clone()),
            status_text: None,
        };
        let _ = self.inner.send_response(channel, response);

        Some(InboundProbeEvent::Response {
            probe_id,
            peer: *peer,
            address: address.clone(),
        })
    }

    pub fn on_outbound_dial_error(
        &mut self,
        peer: Option<PeerId>,
        error: &DialError,
    ) -> Option<InboundProbeEvent> {
        let (probe_id, _, _, channel) = peer.and_then(|p| self.ongoing_inbound.remove(&p))?;
        log::debug!(
            "Dial-back to peer {} failed with error {:?}.",
            peer.unwrap(),
            error
        );
        let response_error = ResponseError::DialError;
        let response = DialResponse {
            result: Err(response_error.clone()),
            status_text: Some("dial failed".to_string()),
        };
        let _ = self.inner.send_response(channel, response);

        Some(InboundProbeEvent::Error {
            probe_id,
            peer: peer.expect("PeerId is present."),
            error: InboundProbeError::Response(response_error),
        })
    }

    // Validate the inbound request and collect the addresses to be dialed.
    fn resolve_inbound_request(
        &mut self,
        sender: PeerId,
        request: DialRequest,
    ) -> Result<Vec<Multiaddr>, (String, ResponseError)> {
        // Update list of throttled clients.
        let i = self.throttled_clients.partition_point(|(_, time)| {
            *time + self.config.throttle_clients_period < Instant::now()
        });
        self.throttled_clients.drain(..i);

        if request.peer_id != sender {
            let status_text = "peer id mismatch".to_string();
            return Err((status_text, ResponseError::BadRequest));
        }

        if self.ongoing_inbound.contains_key(&sender) {
            let status_text = "dial-back already ongoing".to_string();
            return Err((status_text, ResponseError::DialRefused));
        }

        if self.throttled_clients.len() >= self.config.throttle_clients_global_max {
            let status_text = "too many total dials".to_string();
            return Err((status_text, ResponseError::DialRefused));
        }

        let throttled_for_client = self
            .throttled_clients
            .iter()
            .filter(|(p, _)| p == &sender)
            .count();

        if throttled_for_client >= self.config.throttle_clients_peer_max {
            let status_text = "too many dials for peer".to_string();
            return Err((status_text, ResponseError::DialRefused));
        }

        // Obtain an observed address from non-relayed connections.
        let observed_addr = self
            .connected
            .get(&sender)
            .expect("Peer is connected.")
            .values()
            .find_map(|a| a.as_ref())
            .ok_or_else(|| {
                let status_text = "refusing to dial peer with blocked observed address".to_string();
                (status_text, ResponseError::DialRefused)
            })?;

        let mut addrs = Self::filter_valid_addrs(sender, request.addresses, observed_addr);
        addrs.truncate(self.config.max_peer_addresses);

        if addrs.is_empty() {
            let status_text = "no dialable addresses".to_string();
            return Err((status_text, ResponseError::DialRefused));
        }

        Ok(addrs)
    }

    // Filter dial addresses and replace demanded ip with the observed one.
    fn filter_valid_addrs(
        peer: PeerId,
        demanded: Vec<Multiaddr>,
        observed_remote_at: &Multiaddr,
    ) -> Vec<Multiaddr> {
        let observed_ip = match observed_remote_at
            .into_iter()
            .find(|p| matches!(p, Protocol::Ip4(_) | Protocol::Ip6(_)))
        {
            Some(ip) => ip,
            None => return Vec::new(),
        };
        let mut distinct = HashSet::new();
        demanded
            .into_iter()
            .filter_map(|addr| {
                // Replace the demanded ip with the observed one.
                let i = addr
                    .iter()
                    .position(|p| matches!(p, Protocol::Ip4(_) | Protocol::Ip6(_)))?;
                let mut addr = addr.replace(i, |_| Some(observed_ip.clone()))?;

                let is_valid = addr.iter().all(|proto| match proto {
                    Protocol::P2pCircuit => false,
                    Protocol::P2p(hash) => hash == peer.into(),
                    _ => true,
                });

                if !is_valid {
                    return None;
                }
                if !addr.iter().any(|p| matches!(p, Protocol::P2p(_))) {
                    addr.push(Protocol::P2p(peer.into()))
                }
                // Only collect distinct addresses.
                distinct.insert(addr.clone()).then(|| addr)
            })
            .collect()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use std::net::Ipv4Addr;

    fn random_ip<'a>() -> Protocol<'a> {
        Protocol::Ip4(Ipv4Addr::new(
            rand::random(),
            rand::random(),
            rand::random(),
            rand::random(),
        ))
    }
    fn random_port<'a>() -> Protocol<'a> {
        Protocol::Tcp(rand::random())
    }

    #[test]
    fn filter_addresses() {
        let peer_id = PeerId::random();
        let observed_ip = random_ip();
        let observed_addr = Multiaddr::empty()
            .with(observed_ip.clone())
            .with(random_port())
            .with(Protocol::P2p(peer_id.into()));
        // Valid address with matching peer-id
        let demanded_1 = Multiaddr::empty()
            .with(random_ip())
            .with(random_port())
            .with(Protocol::P2p(peer_id.into()));
        // Invalid because peer_id does not match
        let demanded_2 = Multiaddr::empty()
            .with(random_ip())
            .with(random_port())
            .with(Protocol::P2p(PeerId::random().into()));
        // Valid address without peer-id
        let demanded_3 = Multiaddr::empty().with(random_ip()).with(random_port());
        // Invalid because relayed
        let demanded_4 = Multiaddr::empty()
            .with(random_ip())
            .with(random_port())
            .with(Protocol::P2p(PeerId::random().into()))
            .with(Protocol::P2pCircuit)
            .with(Protocol::P2p(peer_id.into()));
        let demanded = vec![
            demanded_1.clone(),
            demanded_2,
            demanded_3.clone(),
            demanded_4,
        ];
        let filtered = AsServer::filter_valid_addrs(peer_id, demanded, &observed_addr);
        let expected_1 = demanded_1
            .replace(0, |_| Some(observed_ip.clone()))
            .unwrap();
        let expected_2 = demanded_3
            .replace(0, |_| Some(observed_ip))
            .unwrap()
            .with(Protocol::P2p(peer_id.into()));
        assert_eq!(filtered, vec![expected_1, expected_2]);
    }
}
