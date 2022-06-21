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

use crate::ResponseError;

use super::{
    Action, AutoNatCodec, Config, DialRequest, DialResponse, Event, HandleInnerEvent, NatStatus,
    ProbeId,
};
use futures::FutureExt;
use futures_timer::Delay;
use instant::Instant;
use libp2p_core::{connection::ConnectionId, Multiaddr, PeerId};
use libp2p_request_response::{
    OutboundFailure, RequestId, RequestResponse, RequestResponseEvent, RequestResponseMessage,
};
use libp2p_swarm::{AddressScore, NetworkBehaviourAction, PollParameters};
use rand::{seq::SliceRandom, thread_rng};
use std::{
    collections::{HashMap, VecDeque},
    task::{Context, Poll},
    time::Duration,
};

/// Outbound probe failed or was aborted.
#[derive(Debug, Clone, PartialEq)]
pub enum OutboundProbeError {
    /// Probe was aborted because no server is known, or all servers
    /// are throttled through [`Config::throttle_server_period`].
    NoServer,
    ///  Probe was aborted because the local peer has no listening or
    /// external addresses.
    NoAddresses,
    /// Sending the dial-back request or receiving a response failed.
    OutboundRequest(OutboundFailure),
    /// The server refused or failed to dial us.
    Response(ResponseError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutboundProbeEvent {
    /// A dial-back request was sent to a remote peer.
    Request {
        probe_id: ProbeId,
        /// Peer to which the request is sent.
        peer: PeerId,
    },
    /// The remote successfully dialed one of our addresses.
    Response {
        probe_id: ProbeId,
        /// Id of the peer that sent the response.
        peer: PeerId,
        /// The address at which the remote succeeded to dial us.
        address: Multiaddr,
    },
    /// The outbound request failed, was rejected, or the remote could dial
    /// none of our addresses.
    Error {
        probe_id: ProbeId,
        /// Id of the peer used for the probe.
        /// `None` if the probe was aborted due to no addresses or no qualified server.
        peer: Option<PeerId>,
        error: OutboundProbeError,
    },
}

/// View over [`super::Behaviour`] in a client role.
pub struct AsClient<'a> {
    pub inner: &'a mut RequestResponse<AutoNatCodec>,
    pub local_peer_id: PeerId,
    pub config: &'a Config,
    pub connected: &'a HashMap<PeerId, HashMap<ConnectionId, Option<Multiaddr>>>,
    pub probe_id: &'a mut ProbeId,

    pub servers: &'a Vec<PeerId>,
    pub throttled_servers: &'a mut Vec<(PeerId, Instant)>,

    pub nat_status: &'a mut NatStatus,
    pub confidence: &'a mut usize,

    pub ongoing_outbound: &'a mut HashMap<RequestId, ProbeId>,

    pub last_probe: &'a mut Option<Instant>,
    pub schedule_probe: &'a mut Delay,
}

impl<'a> HandleInnerEvent for AsClient<'a> {
    fn handle_event(
        &mut self,
        params: &mut impl PollParameters,
        event: RequestResponseEvent<DialRequest, DialResponse>,
    ) -> (VecDeque<Event>, Option<Action>) {
        let mut events = VecDeque::new();
        let mut action = None;
        match event {
            RequestResponseEvent::Message {
                peer,
                message:
                    RequestResponseMessage::Response {
                        request_id,
                        response,
                    },
            } => {
                log::debug!("Outbound dial-back request returned {:?}.", response);

                let probe_id = self
                    .ongoing_outbound
                    .remove(&request_id)
                    .expect("RequestId exists.");

                let event = match response.result.clone() {
                    Ok(address) => OutboundProbeEvent::Response {
                        probe_id,
                        peer,
                        address,
                    },
                    Err(e) => OutboundProbeEvent::Error {
                        probe_id,
                        peer: Some(peer),
                        error: OutboundProbeError::Response(e),
                    },
                };
                events.push_back(Event::OutboundProbe(event));

                if let Some(old) = self.handle_reported_status(response.result.clone().into()) {
                    events.push_back(Event::StatusChanged {
                        old,
                        new: self.nat_status.clone(),
                    });
                }

                if let Ok(address) = response.result {
                    // Update observed address score if it is finite.
                    let score = params
                        .external_addresses()
                        .find_map(|r| (r.addr == address).then(|| r.score))
                        .unwrap_or(AddressScore::Finite(0));
                    if let AddressScore::Finite(finite_score) = score {
                        action = Some(NetworkBehaviourAction::ReportObservedAddr {
                            address,
                            score: AddressScore::Finite(finite_score + 1),
                        });
                    }
                }
            }
            RequestResponseEvent::OutboundFailure {
                peer,
                error,
                request_id,
            } => {
                log::debug!(
                    "Outbound Failure {} when on dial-back request to peer {}.",
                    error,
                    peer
                );
                let probe_id = self
                    .ongoing_outbound
                    .remove(&request_id)
                    .unwrap_or_else(|| self.probe_id.next());

                events.push_back(Event::OutboundProbe(OutboundProbeEvent::Error {
                    probe_id,
                    peer: Some(peer),
                    error: OutboundProbeError::OutboundRequest(error),
                }));

                self.schedule_probe.reset(Duration::ZERO);
            }
            _ => {}
        }
        (events, action)
    }
}

impl<'a> AsClient<'a> {
    pub fn poll_auto_probe(
        &mut self,
        params: &mut impl PollParameters,
        cx: &mut Context<'_>,
    ) -> Poll<OutboundProbeEvent> {
        match self.schedule_probe.poll_unpin(cx) {
            Poll::Ready(()) => {
                self.schedule_probe.reset(self.config.retry_interval);

                let mut addresses: Vec<_> = params.external_addresses().map(|r| r.addr).collect();
                addresses.extend(params.listened_addresses());

                let probe_id = self.probe_id.next();
                let event = match self.do_probe(probe_id, addresses) {
                    Ok(peer) => OutboundProbeEvent::Request { probe_id, peer },
                    Err(error) => {
                        self.handle_reported_status(NatStatus::Unknown);
                        OutboundProbeEvent::Error {
                            probe_id,
                            peer: None,
                            error,
                        }
                    }
                };
                Poll::Ready(event)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    // An inbound connection can indicate that we are public; adjust the delay to the next probe.
    pub fn on_inbound_connection(&mut self) {
        if *self.confidence == self.config.confidence_max {
            if self.nat_status.is_public() {
                self.schedule_next_probe(self.config.refresh_interval * 2);
            } else {
                self.schedule_next_probe(self.config.refresh_interval / 5);
            }
        }
    }

    pub fn on_new_address(&mut self) {
        if !self.nat_status.is_public() {
            // New address could be publicly reachable, trigger retry.
            if *self.confidence > 0 {
                *self.confidence -= 1;
            }
            self.schedule_next_probe(self.config.retry_interval);
        }
    }

    pub fn on_expired_address(&mut self, addr: &Multiaddr) {
        if let NatStatus::Public(public_address) = self.nat_status {
            if public_address == addr {
                *self.confidence = 0;
                *self.nat_status = NatStatus::Unknown;
                self.schedule_next_probe(Duration::ZERO);
            }
        }
    }

    // Select a random server for the probe.
    fn random_server(&mut self) -> Option<PeerId> {
        // Update list of throttled servers.
        let i = self.throttled_servers.partition_point(|(_, time)| {
            *time + self.config.throttle_server_period < Instant::now()
        });
        self.throttled_servers.drain(..i);

        let mut servers: Vec<&PeerId> = self.servers.iter().collect();

        if self.config.use_connected {
            servers.extend(self.connected.iter().filter_map(|(id, addrs)| {
                // Filter servers for which no qualified address is known.
                // This is the case if the connection is relayed or the address is
                // not global (in case of Config::only_global_ips).
                addrs.values().any(|a| a.is_some()).then(|| id)
            }));
        }

        servers.retain(|s| !self.throttled_servers.iter().any(|(id, _)| s == &id));

        servers.choose(&mut thread_rng()).map(|&&p| p)
    }

    // Send a dial-request to a randomly selected server.
    // Returns the server that is used in this probe.
    // `Err` if there are no qualified servers or no addresses.
    fn do_probe(
        &mut self,
        probe_id: ProbeId,
        addresses: Vec<Multiaddr>,
    ) -> Result<PeerId, OutboundProbeError> {
        let _ = self.last_probe.insert(Instant::now());
        if addresses.is_empty() {
            log::debug!("Outbound dial-back request aborted: No dial-back addresses.");
            return Err(OutboundProbeError::NoAddresses);
        }
        let server = match self.random_server() {
            Some(s) => s,
            None => {
                log::debug!("Outbound dial-back request aborted: No qualified server.");
                return Err(OutboundProbeError::NoServer);
            }
        };
        let request_id = self.inner.send_request(
            &server,
            DialRequest {
                peer_id: self.local_peer_id,
                addresses,
            },
        );
        self.throttled_servers.push((server, Instant::now()));
        log::debug!("Send dial-back request to peer {}.", server);
        self.ongoing_outbound.insert(request_id, probe_id);
        Ok(server)
    }

    // Set the delay to the next probe based on the time of our last probe
    // and the specified delay.
    fn schedule_next_probe(&mut self, delay: Duration) {
        let last_probe_instant = match self.last_probe {
            Some(instant) => instant,
            None => {
                return;
            }
        };
        let schedule_next = *last_probe_instant + delay;
        self.schedule_probe
            .reset(schedule_next.saturating_duration_since(Instant::now()));
    }

    // Adapt current confidence and NAT status to the status reported by the latest probe.
    // Return the old status if it flipped.
    fn handle_reported_status(&mut self, reported_status: NatStatus) -> Option<NatStatus> {
        self.schedule_next_probe(self.config.retry_interval);

        if matches!(reported_status, NatStatus::Unknown) {
            return None;
        }

        if reported_status == *self.nat_status {
            if *self.confidence < self.config.confidence_max {
                *self.confidence += 1;
            }
            // Delay with (usually longer) refresh-interval.
            if *self.confidence >= self.config.confidence_max {
                self.schedule_next_probe(self.config.refresh_interval);
            }
            return None;
        }

        if reported_status.is_public() && self.nat_status.is_public() {
            // Different address than the currently assumed public address was reported.
            // Switch address, but don't report as flipped.
            *self.nat_status = reported_status;
            return None;
        }
        if *self.confidence > 0 {
            // Reduce confidence but keep old status.
            *self.confidence -= 1;
            return None;
        }

        log::debug!(
            "Flipped assumed NAT status from {:?} to {:?}",
            self.nat_status,
            reported_status
        );

        let old_status = self.nat_status.clone();
        *self.nat_status = reported_status;

        Some(old_status)
    }
}

impl From<Result<Multiaddr, ResponseError>> for NatStatus {
    fn from(result: Result<Multiaddr, ResponseError>) -> Self {
        match result {
            Ok(addr) => NatStatus::Public(addr),
            Err(ResponseError::DialError) => NatStatus::Private,
            _ => NatStatus::Unknown,
        }
    }
}
