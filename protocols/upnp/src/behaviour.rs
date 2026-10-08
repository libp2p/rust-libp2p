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

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use std::{
    collections::{HashMap, VecDeque, hash_map::Entry::Vacant},
    error::Error,
    hash::Hash,
    net::{self, IpAddr, SocketAddr, SocketAddrV4},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{Future, SinkExt, StreamExt, channel::oneshot};
use futures_timer::Delay;
use igd_next::PortMappingProtocol;
use libp2p_core::{
    Endpoint, Multiaddr, multiaddr,
    transport::{ListenerId, PortUse},
};
use libp2p_swarm::{
    ConnectionDenied, ConnectionId, ExpiredListenAddr, FromSwarm, NetworkBehaviour, NewListenAddr,
    ToSwarm, derive_prelude::PeerId, dummy,
};

use crate::tokio::{Gateway, is_addr_global};

/// HashMap key for port-level mapping state: (protocol, port).
type MappingKey = (PortMappingProtocol, u16);

/// The duration in seconds of a port mapping on the gateway.
const MAPPING_DURATION: u32 = 3600;

/// Renew the Mapping every half of `MAPPING_DURATION` to avoid the port being unmapped.
const MAPPING_TIMEOUT: u64 = MAPPING_DURATION as u64 / 2;

/// Maximum number of retry attempts for failed mappings.
const MAX_RETRY_ATTEMPTS: u32 = 5;

/// Base delay in seconds for exponential backoff (will be multiplied by 2^retry_count).
const BASE_RETRY_DELAY_SECS: u64 = 30;

/// Maximum delay in seconds between retry attempts.
const MAX_RETRY_DELAY_SECS: u64 = 1800;

/// A [`Gateway`] Request.
#[derive(Debug)]
pub(crate) enum GatewayRequest {
    AddMapping { mapping: Mapping, duration: u32 },
    RemoveMapping(Mapping),
}

/// State of an in-flight [`GatewayRequest::AddMapping`] request.
#[derive(Debug)]
enum AddRequestState {
    /// Gateway not yet found; AddMapping will be sent once it becomes available.
    WaitingForGateway,
    /// A previous listener's mapping must be removed before this one can be added.
    WaitingForRemoval,
    /// Request has been sent to the gateway, awaiting response.
    AwaitingResponse {
        /// Number of prior failed attempts for this port.
        retry_count: u32,
    },
    /// Previous attempt failed; waiting before retrying.
    Failed {
        /// Number of attempts so far.
        retry_count: u32,
        /// When to retry.
        next_retry: Delay,
    },
}

#[derive(Debug)]
enum RemoveRequestState {
    Pending(u32),
    AwaitingResponse(u32),
}

/// A [`Gateway`] event.
#[derive(Debug)]
pub(crate) enum GatewayEvent {
    /// Port was successfully mapped.
    Mapped(Mapping),
    /// There was a failure mapping port.
    MapFailure(Mapping, Box<dyn Error + Send + Sync + 'static>),
    /// Port was successfully removed.
    Removed(Mapping),
    /// There was a failure removing the mapped port.
    RemovalFailure(Mapping, Box<dyn Error + Send + Sync + 'static>),
}

/// Mapping of a Protocol and Port on the gateway.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Mapping {
    pub(crate) listener_id: ListenerId,
    pub(crate) protocol: PortMappingProtocol,
    pub(crate) multiaddr: Multiaddr,
    pub(crate) internal_addr: SocketAddr,
}

impl Mapping {
    /// Given the input gateway address, calculate the
    /// open external `Multiaddr`.
    fn external_addr(&self, gateway_addr: IpAddr) -> Multiaddr {
        let addr = match gateway_addr {
            net::IpAddr::V4(ip) => multiaddr::Protocol::Ip4(ip),
            net::IpAddr::V6(ip) => multiaddr::Protocol::Ip6(ip),
        };
        self.multiaddr
            .replace(0, |_| Some(addr))
            .expect("multiaddr should be valid")
    }
}

/// Current state of the UPnP [`Gateway`].
enum GatewayState {
    Searching(oneshot::Receiver<Result<Gateway, Box<dyn std::error::Error + Send + Sync>>>),
    Available(Gateway),
    GatewayNotFound,
    NonRoutableGateway(IpAddr),
}

/// The event produced by `Behaviour`.
#[derive(Debug)]
pub enum Event {
    /// The multiaddress is reachable externally.
    NewExternalAddr {
        /// The local listen address that was mapped.
        local_addr: Multiaddr,
        /// The external address that is reachable.
        external_addr: Multiaddr,
    },
    /// The renewal of the multiaddress on the gateway failed.
    ExpiredExternalAddr {
        /// The local listen address that failed to renew.
        local_addr: Multiaddr,
        /// The external address that is no longer reachable.
        external_addr: Multiaddr,
    },
    /// The IGD gateway was not found.
    GatewayNotFound,
    /// The Gateway is not exposed directly to the public network.
    NonRoutableGateway,
}

/// A [`NetworkBehaviour`] for UPnP port mapping. Automatically tries to map the external port
/// to an internal address on the gateway on a [`FromSwarm::NewListenAddr`].
pub struct Behaviour {
    /// UPnP interface state.
    state: GatewayState,

    /// Active port mappings on the gateway.
    /// The [`Delay`] is the renewal timeout; when it fires, the mapping is re-registered.
    mappings: HashMap<MappingKey, (Mapping, Delay)>,

    /// In-flight AddMapping requests.
    add_requests: HashMap<Mapping, AddRequestState>,

    /// In-flight RemoveMapping requests.
    /// Retains requests that cannot yet fit in the gateway's channel.
    remove_requests: HashMap<Mapping, RemoveRequestState>,

    /// Pending behaviour events to be emitted.
    pending_events: VecDeque<Event>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            state: GatewayState::Searching(crate::tokio::search_gateway()),
            mappings: Default::default(),
            add_requests: Default::default(),
            remove_requests: Default::default(),
            pending_events: VecDeque::new(),
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::NewListenAddr(NewListenAddr {
                listener_id,
                addr: multiaddr,
            }) => {
                let Ok((addr, protocol)) = multiaddr_to_socketaddr_protocol(multiaddr.clone())
                else {
                    tracing::debug!("multiaddress not supported for UPnP {multiaddr}");
                    return;
                };

                if self.mappings.contains_key(&(protocol, addr.port()))
                    || self.add_requests.keys().any(|pending| {
                        pending.protocol == protocol && pending.internal_addr.port() == addr.port()
                    })
                {
                    tracing::debug!(
                        multiaddress=%multiaddr,
                        "port from multiaddress is already mapped on the gateway"
                    );
                    return;
                }

                let mapping = Mapping {
                    listener_id,
                    protocol,
                    internal_addr: addr,
                    multiaddr: multiaddr.clone(),
                };

                match &mut self.state {
                    GatewayState::Searching(_) => {
                        // Gateway not yet found; remember this mapping so it can be sent once the
                        // gateway becomes available.
                        self.add_requests
                            .insert(mapping, AddRequestState::WaitingForGateway);
                    }
                    GatewayState::Available(gateway) => {
                        if self.remove_requests.keys().any(|pending| {
                            pending.protocol == protocol
                                && pending.internal_addr.port() == addr.port()
                        }) {
                            self.add_requests
                                .insert(mapping, AddRequestState::WaitingForRemoval);
                            return;
                        }
                        let duration = MAPPING_DURATION;
                        if let Err(err) = gateway.sender.try_send(GatewayRequest::AddMapping {
                            mapping: mapping.clone(),
                            duration,
                        }) {
                            tracing::debug!(
                                multiaddress=%mapping.multiaddr,
                                "could not request port mapping for multiaddress on the gateway: {}",
                                err
                            );
                        } else {
                            self.add_requests.insert(
                                mapping,
                                AddRequestState::AwaitingResponse { retry_count: 0 },
                            );
                        }
                    }
                    GatewayState::GatewayNotFound => {
                        tracing::debug!(
                            multiaddres=%multiaddr,
                            "network gateway not found, UPnP port mapping of multiaddres discarded"
                        );
                    }
                    GatewayState::NonRoutableGateway(addr) => {
                        tracing::debug!(
                            multiaddress=%multiaddr,
                            network_gateway_ip=%addr,
                            "the network gateway is not exposed to the public network. /
                             UPnP port mapping of multiaddress discarded"
                        );
                    }
                };
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
                listener_id,
                addr: multiaddr,
            }) => {
                let Ok((addr, protocol)) = multiaddr_to_socketaddr_protocol(multiaddr.clone())
                else {
                    tracing::debug!("multiaddress not supported for UPnP {multiaddr}");
                    return;
                };

                let mapping = Mapping {
                    listener_id,
                    protocol,
                    internal_addr: addr,
                    multiaddr: multiaddr.clone(),
                };
                match &mut self.state {
                    GatewayState::Searching(_) => {
                        // Gateway not yet found; cancel the pending AddMapping if present.
                        self.add_requests.remove(&mapping);
                    }
                    GatewayState::Available(gateway) => {
                        let pending = self.add_requests.remove(&mapping);
                        let owns_mapping = self
                            .mappings
                            .get(&(protocol, addr.port()))
                            .is_some_and(|(active, _)| active == &mapping);
                        if owns_mapping {
                            self.mappings.remove(&(protocol, addr.port()));
                            self.pending_events.retain(|event| !matches!(event,
                                Event::NewExternalAddr { local_addr, .. } if local_addr == multiaddr
                            ));
                        }
                        if owns_mapping
                            || matches!(pending, Some(AddRequestState::AwaitingResponse { .. }))
                        {
                            // The gateway serializes requests, so this also removes a late add.
                            if let Err(err) = gateway
                                .sender
                                .try_send(GatewayRequest::RemoveMapping(mapping.clone()))
                            {
                                tracing::debug!(
                                    multiaddress=%mapping.multiaddr,
                                    "could not request port removal for multiaddress on the gateway: {}",
                                    err
                                );
                                self.remove_requests
                                    .insert(mapping, RemoveRequestState::Pending(0));
                            } else {
                                self.remove_requests
                                    .insert(mapping, RemoveRequestState::AwaitingResponse(0));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        libp2p_core::util::unreachable(event)
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>> {
        // If there are pending addresses to be emitted we emit them.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        // Loop through the gateway state so that if it changes from `Searching` to `Available`
        // we poll the pending mapping requests.
        loop {
            match self.state {
                GatewayState::Searching(ref mut fut) => match Pin::new(fut).poll(cx) {
                    Poll::Ready(Ok(result)) => match result {
                        Ok(mut gateway) => {
                            if !is_addr_global(gateway.external_addr) {
                                self.state =
                                    GatewayState::NonRoutableGateway(gateway.external_addr);
                                tracing::debug!(
                                    gateway_address=%gateway.external_addr,
                                    "the gateway is not routable"
                                );
                                return Poll::Ready(ToSwarm::GenerateEvent(
                                    Event::NonRoutableGateway,
                                ));
                            }
                            // Send AddMapping for all addresses that were waiting for
                            // the gateway to become available.
                            let waiting: Vec<Mapping> = self
                                .add_requests
                                .iter()
                                .filter(|(_, s)| matches!(s, AddRequestState::WaitingForGateway))
                                .map(|(m, _)| m.clone())
                                .collect();
                            for mapping in waiting {
                                let duration = MAPPING_DURATION;
                                if let Err(err) =
                                    gateway.sender.try_send(GatewayRequest::AddMapping {
                                        mapping: mapping.clone(),
                                        duration,
                                    })
                                {
                                    tracing::debug!(
                                        multiaddress=%mapping.multiaddr,
                                        "could not request port mapping for multiaddress on the gateway: {}",
                                        err
                                    );
                                    self.add_requests.remove(&mapping);
                                } else {
                                    self.add_requests.insert(
                                        mapping,
                                        AddRequestState::AwaitingResponse { retry_count: 0 },
                                    );
                                }
                            }
                            self.state = GatewayState::Available(gateway);
                        }
                        Err(err) => {
                            tracing::debug!("could not find gateway: {err}");
                            self.state = GatewayState::GatewayNotFound;
                            return Poll::Ready(ToSwarm::GenerateEvent(Event::GatewayNotFound));
                        }
                    },
                    Poll::Ready(Err(err)) => {
                        // The sender channel has been dropped. This typically indicates a shutdown
                        // process is underway.
                        tracing::debug!("sender has been dropped: {err}");
                        self.state = GatewayState::GatewayNotFound;
                        return Poll::Ready(ToSwarm::GenerateEvent(Event::GatewayNotFound));
                    }
                    Poll::Pending => return Poll::Pending,
                },
                GatewayState::Available(ref mut gateway) => {
                    for (mapping, state) in &mut self.remove_requests {
                        if let RemoveRequestState::Pending(retry_count) = state
                            && matches!(gateway.sender.poll_ready_unpin(cx), Poll::Ready(Ok(())))
                            && gateway
                                .sender
                                .try_send(GatewayRequest::RemoveMapping(mapping.clone()))
                                .is_ok()
                        {
                            *state = RemoveRequestState::AwaitingResponse(*retry_count);
                        }
                    }
                    // Poll pending mapping requests.
                    if let Poll::Ready(Some(result)) = gateway.receiver.poll_next_unpin(cx) {
                        match result {
                            GatewayEvent::Mapped(mapping) => {
                                if !matches!(
                                    self.add_requests.get(&mapping),
                                    Some(AddRequestState::AwaitingResponse { .. })
                                ) {
                                    // A canceled request's removal is already queued.
                                    continue;
                                }
                                self.add_requests.remove(&mapping);

                                let key = (mapping.protocol, mapping.internal_addr.port());

                                if let Vacant(e) = self.mappings.entry(key) {
                                    // New mapping or retry success: insert the active entry.
                                    e.insert((
                                        mapping.clone(),
                                        Delay::new(Duration::from_secs(MAPPING_TIMEOUT)),
                                    ));
                                    let external_multiaddr =
                                        mapping.external_addr(gateway.external_addr);
                                    self.pending_events.push_back(Event::NewExternalAddr {
                                        local_addr: mapping.multiaddr,
                                        external_addr: external_multiaddr.clone(),
                                    });
                                    tracing::debug!(
                                        address=%mapping.internal_addr,
                                        protocol=%mapping.protocol,
                                        "successfully mapped UPnP for protocol"
                                    );
                                    return Poll::Ready(ToSwarm::ExternalAddrConfirmed(
                                        external_multiaddr,
                                    ));
                                } else {
                                    // Renewal: refresh the timeout in-place.
                                    if let Some((_, timeout)) = self.mappings.get_mut(&key) {
                                        *timeout = Delay::new(Duration::from_secs(MAPPING_TIMEOUT));
                                    }
                                    tracing::debug!(
                                        address=%mapping.internal_addr,
                                        protocol=%mapping.protocol,
                                        "successfully renewed UPnP mapping for protocol"
                                    );
                                }
                            }
                            GatewayEvent::MapFailure(mapping, err) => {
                                let retry_count = match self.add_requests.get(&mapping) {
                                    Some(AddRequestState::AwaitingResponse { retry_count }) => {
                                        *retry_count
                                    }
                                    other => {
                                        tracing::warn!(
                                            mapping=?mapping,
                                            existing_state=?other,
                                            "received MapFailure for mapping not in AwaitingResponse state, ignoring"
                                        );
                                        continue;
                                    }
                                };
                                self.add_requests.remove(&mapping);

                                let key = (mapping.protocol, mapping.internal_addr.port());
                                // Remove the Active entry if present (renewal failure).
                                let was_active = self.mappings.remove(&key).is_some();

                                let new_retry_count = retry_count + 1;
                                if new_retry_count < MAX_RETRY_ATTEMPTS {
                                    let delay_secs = std::cmp::min(
                                        BASE_RETRY_DELAY_SECS
                                            .saturating_mul(2_u64.pow(retry_count)),
                                        MAX_RETRY_DELAY_SECS,
                                    );
                                    self.add_requests.insert(
                                        mapping.clone(),
                                        AddRequestState::Failed {
                                            retry_count: new_retry_count,
                                            next_retry: Delay::new(Duration::from_secs(delay_secs)),
                                        },
                                    );
                                } else {
                                    tracing::warn!(
                                        address=%mapping.internal_addr,
                                        protocol=%mapping.protocol,
                                        "giving up on UPnP mapping after {new_retry_count} attempts"
                                    );
                                    // In-flight gateway requests entry already removed; leave it
                                    // gone.
                                }

                                if was_active {
                                    tracing::debug!(
                                        address=%mapping.internal_addr,
                                        protocol=%mapping.protocol,
                                        "failed to remap UPnP mapped for protocol: {err}"
                                    );
                                    let external_multiaddr =
                                        mapping.external_addr(gateway.external_addr);
                                    self.pending_events.push_back(Event::ExpiredExternalAddr {
                                        local_addr: mapping.multiaddr,
                                        external_addr: external_multiaddr.clone(),
                                    });
                                    return Poll::Ready(ToSwarm::ExternalAddrExpired(
                                        external_multiaddr,
                                    ));
                                } else {
                                    tracing::debug!(
                                        address=%mapping.internal_addr,
                                        protocol=%mapping.protocol,
                                        retry_count=%new_retry_count,
                                        "failed to map UPnP mapped for protocol: {err}"
                                    );
                                }
                            }
                            GatewayEvent::Removed(mapping) => {
                                tracing::debug!(
                                    address=%mapping.internal_addr,
                                    protocol=%mapping.protocol,
                                    "successfully removed UPnP mapping for protocol"
                                );
                                self.remove_requests.remove(&mapping);
                            }
                            GatewayEvent::RemovalFailure(mapping, err) => {
                                let Some(RemoveRequestState::AwaitingResponse(retry_count)) =
                                    self.remove_requests.remove(&mapping)
                                else {
                                    tracing::warn!(
                                        mapping=?mapping,
                                        "received RemovalFailure for unknown mapping, ignoring"
                                    );
                                    continue;
                                };
                                let new_retry_count = retry_count + 1;
                                if new_retry_count < MAX_RETRY_ATTEMPTS {
                                    tracing::debug!(
                                        address=%mapping.internal_addr,
                                        protocol=%mapping.protocol,
                                        retry_count=%new_retry_count,
                                        "could not remove UPnP mapping for protocol, retrying: {err}"
                                    );
                                    self.remove_requests.insert(
                                        mapping,
                                        RemoveRequestState::Pending(new_retry_count),
                                    );
                                    continue;
                                } else {
                                    tracing::warn!(
                                        address=%mapping.internal_addr,
                                        protocol=%mapping.protocol,
                                        "giving up on UPnP removal after {new_retry_count} attempts: {err}"
                                    );
                                    // Entry already removed; leave it gone.
                                }
                            }
                        }
                    }

                    for (mapping, state) in &mut self.add_requests {
                        if matches!(state, AddRequestState::WaitingForRemoval)
                            && !self.remove_requests.keys().any(|pending| {
                                pending.protocol == mapping.protocol
                                    && pending.internal_addr.port() == mapping.internal_addr.port()
                            })
                            && matches!(gateway.sender.poll_ready_unpin(cx), Poll::Ready(Ok(())))
                            && gateway
                                .sender
                                .try_send(GatewayRequest::AddMapping {
                                    mapping: mapping.clone(),
                                    duration: MAPPING_DURATION,
                                })
                                .is_ok()
                        {
                            *state = AddRequestState::AwaitingResponse { retry_count: 0 };
                        }
                    }
                    // Renew expired and request inactive mappings.
                    renew_mappings(&mut self.mappings, &mut self.add_requests, gateway, cx);
                    return Poll::Pending;
                }
                _ => return Poll::Pending,
            }
        }
    }
}

/// Renew expiring active mappings and retry failed ones.
fn renew_mappings(
    mappings: &mut HashMap<MappingKey, (Mapping, Delay)>,
    add_requests: &mut HashMap<Mapping, AddRequestState>,
    gateway: &mut Gateway,
    cx: &mut Context<'_>,
) {
    for (mapping, timeout) in mappings.values_mut() {
        // Skip if a request for this mapping is already in-flight.
        if add_requests.contains_key(mapping) {
            continue;
        }
        if Pin::new(timeout).poll(cx).is_ready() {
            if let Err(err) = gateway.sender.try_send(GatewayRequest::AddMapping {
                mapping: mapping.clone(),
                duration: MAPPING_DURATION,
            }) {
                tracing::debug!(
                    multiaddress=%mapping.multiaddr,
                    "could not request port mapping for multiaddress on the gateway: {}",
                    err
                );
            } else {
                add_requests.insert(
                    mapping.clone(),
                    AddRequestState::AwaitingResponse { retry_count: 0 },
                );
            }
        }
    }

    // Retry failed mappings whose back-off delay has elapsed.
    let to_retry: Vec<Mapping> = add_requests
        .iter_mut()
        .filter_map(|(mapping, state)| {
            if let AddRequestState::Failed { next_retry, .. } = state
                && Pin::new(next_retry).poll(cx).is_ready()
            {
                return Some(mapping.clone());
            }
            None
        })
        .collect();
    for mapping in to_retry {
        if let Some(AddRequestState::Failed { retry_count, .. }) = add_requests.remove(&mapping) {
            if let Err(err) = gateway.sender.try_send(GatewayRequest::AddMapping {
                mapping: mapping.clone(),
                duration: MAPPING_DURATION,
            }) {
                tracing::debug!(
                    multiaddress=%mapping.multiaddr,
                    retry_count=%retry_count,
                    "could not retry port mapping for multiaddress on the gateway: {}",
                    err
                );
                // Entry already removed; drop it rather than re-inserting.
            } else {
                add_requests.insert(mapping, AddRequestState::AwaitingResponse { retry_count });
            }
        }
    }
}

/// Extracts a [`SocketAddrV4`] and [`PortMappingProtocol`] from a given [`Multiaddr`].
///
/// Fails if the given [`Multiaddr`] does not begin with an IP
/// protocol encapsulating a TCP or UDP port.
fn multiaddr_to_socketaddr_protocol(
    addr: Multiaddr,
) -> Result<(SocketAddr, PortMappingProtocol), ()> {
    let mut iter = addr.into_iter();
    match iter.next() {
        // Idg only supports Ipv4.
        Some(multiaddr::Protocol::Ip4(ipv4)) if ipv4.is_private() => match iter.next() {
            Some(multiaddr::Protocol::Tcp(port)) => {
                return Ok((
                    SocketAddr::V4(SocketAddrV4::new(ipv4, port)),
                    PortMappingProtocol::TCP,
                ));
            }
            Some(multiaddr::Protocol::Udp(port)) => {
                return Ok((
                    SocketAddr::V4(SocketAddrV4::new(ipv4, port)),
                    PortMappingProtocol::UDP,
                ));
            }
            _ => {}
        },
        _ => {}
    }
    Err(())
}

#[cfg(test)]
mod tests {
    use std::{io::Error, net::Ipv4Addr, task::Waker};

    use futures::channel::mpsc;

    use super::*;

    fn build_behaviour_with_gateway() -> (
        Behaviour,
        mpsc::Sender<GatewayEvent>,
        mpsc::Receiver<GatewayRequest>,
    ) {
        let (req_tx, req_rx) = mpsc::channel(10);
        let (event_tx, event_rx) = mpsc::channel(10);
        let gateway = crate::tokio::Gateway {
            sender: req_tx,
            receiver: event_rx,
            external_addr: "203.0.113.1".parse().unwrap(), /* TEST-NET-3 (RFC 5737), passes
                                                            * is_addr_global() */
        };
        let behaviour = Behaviour {
            state: GatewayState::Available(gateway),
            mappings: Default::default(),
            add_requests: Default::default(),
            remove_requests: Default::default(),
            pending_events: VecDeque::new(),
        };
        (behaviour, event_tx, req_rx)
    }

    fn build_mapping(listener_id: ListenerId, ip: Ipv4Addr, port: u16) -> Mapping {
        let multiaddr: Multiaddr = format!("/ip4/{ip}/tcp/{port}").parse().unwrap();
        Mapping {
            listener_id,
            protocol: PortMappingProtocol::TCP,
            internal_addr: SocketAddr::new(IpAddr::V4(ip), port),
            multiaddr,
        }
    }

    #[test]
    fn expired_pending_mapping_is_removed_without_confirmation() {
        for renewal in [false, true] {
            let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
            let mapping = build_mapping(ListenerId::next(), "192.168.1.100".parse().unwrap(), 9000);
            let mut cx = Context::from_waker(Waker::noop());
            behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
                listener_id: mapping.listener_id,
                addr: &mapping.multiaddr,
            }));
            assert!(matches!(
                req_rx.poll_next_unpin(&mut cx),
                Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
            ));
            if renewal {
                behaviour.mappings.insert(
                    (mapping.protocol, mapping.internal_addr.port()),
                    (
                        mapping.clone(),
                        Delay::new(Duration::from_secs(MAPPING_TIMEOUT)),
                    ),
                );
            }
            behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
                listener_id: mapping.listener_id,
                addr: &mapping.multiaddr,
            }));
            assert!(behaviour.add_requests.is_empty());
            assert!(
                matches!(req_rx.poll_next_unpin(&mut cx), Poll::Ready(Some(GatewayRequest::RemoveMapping(ref removed))) if removed == &mapping)
            );

            event_tx
                .try_send(GatewayEvent::Mapped(mapping.clone()))
                .unwrap();
            assert!(behaviour.poll(&mut cx).is_pending());
            assert!(behaviour.mappings.is_empty());
            assert!(behaviour.pending_events.is_empty());
            event_tx.try_send(GatewayEvent::Removed(mapping)).unwrap();
            drain_poll(&mut behaviour, &mut cx);
            assert!(behaviour.remove_requests.is_empty());
        }
    }

    #[test]
    fn expired_failed_mapping_is_not_retried() {
        let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
        let mapping = build_mapping(ListenerId::next(), "192.168.1.100".parse().unwrap(), 9000);
        let mut cx = Context::from_waker(Waker::noop());
        behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
            listener_id: mapping.listener_id,
            addr: &mapping.multiaddr,
        }));
        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
        ));
        event_tx
            .try_send(GatewayEvent::MapFailure(
                mapping.clone(),
                Box::new(Error::other("mock")),
            ))
            .unwrap();
        drain_poll(&mut behaviour, &mut cx);
        behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id: mapping.listener_id,
            addr: &mapping.multiaddr,
        }));
        assert!(behaviour.add_requests.is_empty());
        assert!(behaviour.poll(&mut cx).is_pending());
        assert!(req_rx.poll_next_unpin(&mut cx).is_pending());
    }

    #[test]
    fn non_owner_expiration_does_not_remove_an_active_mapping() {
        for confirmed in [false, true] {
            let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
            let mapping = build_mapping(ListenerId::next(), "192.168.1.100".parse().unwrap(), 9000);
            let mut cx = Context::from_waker(Waker::noop());
            behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
                listener_id: mapping.listener_id,
                addr: &mapping.multiaddr,
            }));
            assert!(matches!(
                req_rx.poll_next_unpin(&mut cx),
                Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
            ));
            if confirmed {
                event_tx
                    .try_send(GatewayEvent::Mapped(mapping.clone()))
                    .unwrap();
                drain_poll(&mut behaviour, &mut cx);
            }

            for (listener_id, addr) in [
                (ListenerId::next(), mapping.multiaddr.clone()),
                (
                    mapping.listener_id,
                    "/ip4/10.0.0.5/tcp/9000".parse().unwrap(),
                ),
            ] {
                behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
                    listener_id,
                    addr: &addr,
                }));
                behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
                    listener_id,
                    addr: &addr,
                }));
                if confirmed {
                    assert_eq!(
                        behaviour
                            .mappings
                            .get(&(mapping.protocol, 9000))
                            .map(|(m, _)| m),
                        Some(&mapping)
                    );
                } else {
                    assert!(behaviour.add_requests.contains_key(&mapping));
                }
                assert!(req_rx.poll_next_unpin(&mut cx).is_pending());
            }
        }
    }

    #[test]
    fn replacement_waits_for_backpressured_removal_and_retry() {
        for same_mapping in [false, true] {
            let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
            let mapping = build_mapping(ListenerId::next(), "192.168.1.100".parse().unwrap(), 9000);
            let replacement = if same_mapping {
                mapping.clone()
            } else {
                build_mapping(ListenerId::next(), "10.0.0.5".parse().unwrap(), 9000)
            };
            let mut cx = Context::from_waker(Waker::noop());
            behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
                listener_id: mapping.listener_id,
                addr: &mapping.multiaddr,
            }));
            assert!(matches!(
                req_rx.poll_next_unpin(&mut cx),
                Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
            ));
            // Exhaust the real request channel's capacity while the add is in flight.
            if let GatewayState::Available(gateway) = &mut behaviour.state {
                while gateway
                    .sender
                    .try_send(GatewayRequest::AddMapping {
                        mapping: mapping.clone(),
                        duration: MAPPING_DURATION,
                    })
                    .is_ok()
                {}
            }
            behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
                listener_id: mapping.listener_id,
                addr: &mapping.multiaddr,
            }));
            behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
                listener_id: replacement.listener_id,
                addr: &replacement.multiaddr,
            }));
            event_tx
                .try_send(GatewayEvent::Mapped(mapping.clone()))
                .unwrap();
            assert!(behaviour.poll(&mut cx).is_pending());
            assert!(behaviour.mappings.is_empty());
            while let Poll::Ready(Some(request)) = req_rx.poll_next_unpin(&mut cx) {
                assert!(
                    matches!(request, GatewayRequest::AddMapping { mapping: ref added, .. } if added == &mapping)
                );
            }
            assert!(behaviour.poll(&mut cx).is_pending());
            assert!(
                matches!(req_rx.poll_next_unpin(&mut cx), Poll::Ready(Some(GatewayRequest::RemoveMapping(ref removed))) if removed == &mapping)
            );
            assert!(req_rx.poll_next_unpin(&mut cx).is_pending());
            event_tx
                .try_send(GatewayEvent::RemovalFailure(
                    mapping.clone(),
                    Box::new(Error::other("mock")),
                ))
                .unwrap();
            assert!(behaviour.poll(&mut cx).is_pending());
            assert!(
                matches!(req_rx.poll_next_unpin(&mut cx), Poll::Ready(Some(GatewayRequest::RemoveMapping(ref removed))) if removed == &mapping)
            );
            assert!(req_rx.poll_next_unpin(&mut cx).is_pending());
            event_tx.try_send(GatewayEvent::Removed(mapping)).unwrap();
            assert!(behaviour.poll(&mut cx).is_pending());
            assert!(
                matches!(req_rx.poll_next_unpin(&mut cx), Poll::Ready(Some(GatewayRequest::AddMapping { mapping: ref added, .. })) if added == &replacement)
            );
            event_tx
                .try_send(GatewayEvent::Mapped(replacement.clone()))
                .unwrap();
            assert!(matches!(
                behaviour.poll(&mut cx),
                Poll::Ready(ToSwarm::ExternalAddrConfirmed(_))
            ));
            assert_eq!(
                behaviour
                    .mappings
                    .get(&(replacement.protocol, 9000))
                    .map(|(m, _)| m),
                Some(&replacement)
            );
        }
    }

    fn drain_poll(behaviour: &mut Behaviour, cx: &mut Context<'_>) {
        loop {
            if behaviour.poll(cx).is_pending() {
                break;
            }
        }
    }

    /// A new listen address is mapped on the gateway, then the listener expires and
    /// the mapping is removed.
    #[test]
    fn new_mapping_then_removed() {
        let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
        let listener_id = ListenerId::next();
        let ip: Ipv4Addr = "192.168.1.100".parse().unwrap();
        let port = 9000u16;

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // NewListenAddr fires → AddMapping enqueued to the gateway.
        let addr: Multiaddr = format!("/ip4/{ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
            listener_id,
            addr: &addr,
        }));
        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
        ));

        // The gateway confirms the mapping → Active.
        let mapping = build_mapping(listener_id, ip, port);
        event_tx
            .try_send(GatewayEvent::Mapped(mapping.clone()))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);
        assert!(
            behaviour
                .mappings
                .contains_key(&(PortMappingProtocol::TCP, port))
        );

        // The listener expires → RemoveMapping enqueued to the gateway.
        behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id,
            addr: &addr,
        }));
        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::RemoveMapping(_)))
        ));

        // The gateway confirms the removal → mappings cleared.
        event_tx
            .try_send(GatewayEvent::Removed(mapping))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);
        assert!(behaviour.mappings.is_empty());
        assert!(behaviour.add_requests.is_empty());
        assert!(behaviour.remove_requests.is_empty());
    }

    /// A new listen address is requested to be mapped, but the gateway returns MapFailure.
    #[test]
    fn new_mapping_map_failure() {
        let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
        let listener_id = ListenerId::next();
        let ip: Ipv4Addr = "192.168.1.100".parse().unwrap();
        let port = 9000u16;

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // NewListenAddr fires → AddMapping enqueued to the gateway.
        let addr: Multiaddr = format!("/ip4/{ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
            listener_id,
            addr: &addr,
        }));
        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
        ));

        // The gateway returns MapFailure → Failed state.
        let mapping = build_mapping(listener_id, ip, port);
        event_tx
            .try_send(GatewayEvent::MapFailure(
                mapping.clone(),
                Box::new(Error::other("mock failure")),
            ))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);
        assert!(behaviour.mappings.is_empty());
        assert!(matches!(
            behaviour.add_requests.get(&mapping),
            Some(AddRequestState::Failed { retry_count: 1, .. })
        ));
    }

    /// Tests that mapping state and in-flight gateway requests are managed correctly
    /// when a network interface changes (e.g. Wi-Fi → Ethernet): ExpiredListenAddr
    /// for the old address is followed by NewListenAddr for the new address on the
    /// same listener, then the gateway confirms removal of the old mapping and
    /// reports that mapping the new address failed.
    #[test]
    fn network_interface_change() {
        let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
        let listener_id = ListenerId::next();
        let old_ip: Ipv4Addr = "192.168.1.100".parse().unwrap();
        let new_ip: Ipv4Addr = "10.0.0.5".parse().unwrap();
        let port = 9000u16;

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // The port is already actively mapped on the gateway for old_ip.
        let old_mapping = build_mapping(listener_id, old_ip, port);
        behaviour.mappings.insert(
            (old_mapping.protocol, old_mapping.internal_addr.port()),
            (
                old_mapping.clone(),
                Delay::new(Duration::from_secs(MAPPING_TIMEOUT)),
            ),
        );

        // The old interface goes away.
        let old_addr: Multiaddr = format!("/ip4/{old_ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id,
            addr: &old_addr,
        }));
        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::RemoveMapping(_)))
        ));

        // The new interface comes up.
        let new_addr: Multiaddr = format!("/ip4/{new_ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
            listener_id,
            addr: &new_addr,
        }));
        assert!(req_rx.poll_next_unpin(&mut cx).is_pending());

        // The gateway confirms the removal of old_ip's mapping.
        event_tx
            .try_send(GatewayEvent::Removed(old_mapping))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);

        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
        ));

        // The gateway reports that mapping new_ip failed.
        let new_mapping = build_mapping(listener_id, new_ip, port);
        event_tx
            .try_send(GatewayEvent::MapFailure(
                new_mapping,
                Box::new(Error::other("mock failure")),
            ))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);

        assert!(behaviour.mappings.is_empty());
        let new_mapping = build_mapping(listener_id, new_ip, port);
        assert!(matches!(
            behaviour.add_requests.get(&new_mapping),
            Some(AddRequestState::Failed { retry_count: 1, .. })
        ));
    }

    /// Tests that mapping state and in-flight gateway requests are managed correctly
    /// when a network interface changes and the gateway successfully maps the new address:
    /// the new mapping becomes active after the old one is removed.
    #[test]
    fn network_interface_change_new_mapping_established() {
        let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
        let listener_id = ListenerId::next();
        let old_ip: Ipv4Addr = "192.168.1.100".parse().unwrap();
        let new_ip: Ipv4Addr = "10.0.0.5".parse().unwrap();
        let port = 9000u16;

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // The port is already actively mapped on the gateway for old_ip.
        let old_mapping = build_mapping(listener_id, old_ip, port);
        behaviour.mappings.insert(
            (old_mapping.protocol, old_mapping.internal_addr.port()),
            (
                old_mapping.clone(),
                Delay::new(Duration::from_secs(MAPPING_TIMEOUT)),
            ),
        );

        // The old interface goes away.
        let old_addr: Multiaddr = format!("/ip4/{old_ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id,
            addr: &old_addr,
        }));
        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::RemoveMapping(_)))
        ));

        // The new interface comes up.
        let new_addr: Multiaddr = format!("/ip4/{new_ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
            listener_id,
            addr: &new_addr,
        }));
        assert!(req_rx.poll_next_unpin(&mut cx).is_pending());

        // The gateway confirms the removal of old_ip's mapping.
        event_tx
            .try_send(GatewayEvent::Removed(old_mapping))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);

        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::AddMapping { .. }))
        ));

        // The gateway successfully maps new_ip.
        let new_mapping = build_mapping(listener_id, new_ip, port);
        event_tx
            .try_send(GatewayEvent::Mapped(new_mapping.clone()))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);

        assert_eq!(
            behaviour
                .mappings
                .get(&(PortMappingProtocol::TCP, port))
                .map(|(m, _)| m),
            Some(&new_mapping)
        );
        assert!(behaviour.add_requests.is_empty());
        assert!(behaviour.remove_requests.is_empty());
    }

    /// Tests that mapping state and in-flight gateway requests are managed correctly
    /// when a listener with multiple addresses (same port, different IPs) is closed:
    /// ExpiredListenAddr fires once per address on the same listener, but only the
    /// first one triggers a RemoveMapping request since only one port mapping can be
    /// active on the gateway at a time.
    #[test]
    fn listener_with_multiple_addresses() {
        let (mut behaviour, mut event_tx, mut req_rx) = build_behaviour_with_gateway();
        let listener_id = ListenerId::next();
        let first_ip: Ipv4Addr = "192.168.1.100".parse().unwrap();
        let second_ip: Ipv4Addr = "10.0.0.5".parse().unwrap();
        let port = 9000u16;

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        // The port is already actively mapped on the gateway for first_ip.
        let mapping = build_mapping(listener_id, first_ip, port);
        behaviour.mappings.insert(
            (mapping.protocol, mapping.internal_addr.port()),
            (
                mapping.clone(),
                Delay::new(Duration::from_secs(MAPPING_TIMEOUT)),
            ),
        );

        // ExpiredListenAddr fires for the first address → RemoveMapping enqueued.
        let first_addr: Multiaddr = format!("/ip4/{first_ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id,
            addr: &first_addr,
        }));
        assert!(matches!(
            req_rx.poll_next_unpin(&mut cx),
            Poll::Ready(Some(GatewayRequest::RemoveMapping(_)))
        ));

        // ExpiredListenAddr fires for the second address on the same listener.
        // The port is already being removed, so no additional gateway request is sent.
        let second_addr: Multiaddr = format!("/ip4/{second_ip}/tcp/{port}").parse().unwrap();
        behaviour.on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id,
            addr: &second_addr,
        }));
        assert!(matches!(req_rx.poll_next_unpin(&mut cx), Poll::Pending));

        // The gateway confirms the removal.
        event_tx
            .try_send(GatewayEvent::Removed(mapping))
            .expect("channel should have capacity");
        drain_poll(&mut behaviour, &mut cx);

        assert!(behaviour.mappings.is_empty());
        assert!(behaviour.add_requests.is_empty());
        assert!(behaviour.remove_requests.is_empty());
    }
}
