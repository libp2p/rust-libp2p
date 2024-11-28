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
    borrow::Borrow,
    collections::{HashMap, VecDeque},
    error::Error,
    hash::{Hash, Hasher},
    net::{self, IpAddr, SocketAddr, SocketAddrV4},
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot, Future, StreamExt};
use futures_timer::Delay;
use igd_next::PortMappingProtocol;
use libp2p_core::{
    multiaddr,
    transport::{ListenerId, PortUse},
    Endpoint, Multiaddr,
};
use libp2p_swarm::{
    derive_prelude::PeerId, dummy, ConnectionDenied, ConnectionId, ExpiredListenAddr, FromSwarm,
    NetworkBehaviour, NewListenAddr, ToSwarm,
};

use crate::tokio::{is_addr_global, Gateway};

/// The duration in seconds of a port mapping on the gateway.
const MAPPING_DURATION: u32 = 3600;

/// Renew the Mapping every half of `MAPPING_DURATION` to avoid the port being unmapped.
const MAPPING_TIMEOUT: u64 = MAPPING_DURATION as u64 / 2;

/// A [`Gateway`] Request.
#[derive(Debug)]
pub(crate) enum GatewayRequest {
    AddMapping { mapping: Mapping, duration: u32 },
    RemoveMapping(Mapping),
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
#[derive(Debug, Clone)]
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

impl Hash for Mapping {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.listener_id.hash(state);
    }
}

impl PartialEq for Mapping {
    fn eq(&self, other: &Self) -> bool {
        self.listener_id == other.listener_id
    }
}

impl Eq for Mapping {}

impl Borrow<ListenerId> for Mapping {
    fn borrow(&self) -> &ListenerId {
        &self.listener_id
    }
}

/// Current state of a [`Mapping`].
#[derive(Debug)]
enum MappingState {
    /// Port mapping is inactive, will be requested or re-requested on the next iteration.
    Inactive,
    /// Port mapping/removal has been requested on the gateway.
    Pending,
    /// Port mapping is active with the inner timeout.
    Active(Delay),
    /// Port mapping failed, we will try again.
    Failed,
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
    NewExternalAddr(Multiaddr),
    /// The renewal of the multiaddress on the gateway failed.
    ExpiredExternalAddr(Multiaddr),
    /// The IGD gateway was not found.
    GatewayNotFound,
    /// The Gateway is not exposed directly to the public network.
    NonRoutableGateway,
}

/// A list of port mappings and its state.
#[derive(Debug, Default)]
struct MappingList(HashMap<Mapping, MappingState>);

impl Deref for MappingList {
    type Target = HashMap<Mapping, MappingState>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MappingList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl MappingList {
    /// Queue for renewal the current mapped ports on the `Gateway` that are expiring,
    /// and try to activate the inactive.
    fn renew(&mut self, gateway: &mut Gateway, cx: &mut Context<'_>) {
        for (mapping, state) in self.iter_mut() {
            match state {
                MappingState::Inactive | MappingState::Failed => {
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
                    }
                    *state = MappingState::Pending;
                }
                MappingState::Active(timeout) => {
                    if Pin::new(timeout).poll(cx).is_ready() {
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
                        }
                    }
                }
                MappingState::Pending => {}
            }
        }
    }
}

/// A [`NetworkBehaviour`] for UPnP port mapping. Automatically tries to map the external port
/// to an internal address on the gateway on a [`FromSwarm::NewListenAddr`].
pub struct Behaviour {
    /// UPnP interface state.
    state: GatewayState,

    /// List of port mappings.
    mappings: MappingList,

    /// Pending behaviour events to be emitted.
    pending_events: VecDeque<Event>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            state: GatewayState::Searching(crate::tokio::search_gateway()),
            mappings: Default::default(),
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
                let (addr, protocol) = match multiaddr_to_socketaddr_protocol(multiaddr.clone()) {
                    Ok(addr_port) => addr_port,
                    Err(()) => {
                        tracing::debug!("multiaddress not supported for UPnP {multiaddr}");
                        return;
                    }
                };

                if let Some((mapping, _state)) = self
                    .mappings
                    .iter()
                    .find(|(mapping, _state)| mapping.internal_addr.port() == addr.port())
                {
                    tracing::debug!(
                        multiaddress=%multiaddr,
                        mapped_multiaddress=%mapping.multiaddr,
                        "port from multiaddress is already being mapped"
                    );
                    return;
                }

                match &mut self.state {
                    GatewayState::Searching(_) => {
                        // As the gateway is not yet available we add the mapping with
                        // `MappingState::Inactive` so that when and if it
                        // becomes available we map it.
                        self.mappings.insert(
                            Mapping {
                                listener_id,
                                protocol,
                                internal_addr: addr,
                                multiaddr: multiaddr.clone(),
                            },
                            MappingState::Inactive,
                        );
                    }
                    GatewayState::Available(ref mut gateway) => {
                        let mapping = Mapping {
                            listener_id,
                            protocol,
                            internal_addr: addr,
                            multiaddr: multiaddr.clone(),
                        };

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
                        }

                        self.mappings.insert(mapping, MappingState::Pending);
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
                addr: _addr,
            }) => {
                if let GatewayState::Available(ref mut gateway) = &mut self.state {
                    if let Some((mapping, _state)) = self.mappings.remove_entry(&listener_id) {
                        if let Err(err) = gateway
                            .sender
                            .try_send(GatewayRequest::RemoveMapping(mapping.clone()))
                        {
                            tracing::debug!(
                                multiaddress=%mapping.multiaddr,
                                "could not request port removal for multiaddress on the gateway: {}",
                                err
                            );
                        }
                        self.mappings.insert(mapping, MappingState::Pending);
                    }
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
                    Poll::Ready(result) => {
                        match result.expect("sender shouldn't have been dropped") {
                            Ok(gateway) => {
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
                                self.state = GatewayState::Available(gateway);
                            }
                            Err(err) => {
                                tracing::debug!("could not find gateway: {err}");
                                self.state = GatewayState::GatewayNotFound;
                                return Poll::Ready(ToSwarm::GenerateEvent(Event::GatewayNotFound));
                            }
                        }
                    }
                    Poll::Pending => return Poll::Pending,
                },
                GatewayState::Available(ref mut gateway) => {
                    // Poll pending mapping requests.
                    if let Poll::Ready(Some(result)) = gateway.receiver.poll_next_unpin(cx) {
                        match result {
                            GatewayEvent::Mapped(mapping) => {
                                let new_state = MappingState::Active(Delay::new(
                                    Duration::from_secs(MAPPING_TIMEOUT),
                                ));

                                match self
                                    .mappings
                                    .insert(mapping.clone(), new_state)
                                    .expect("mapping should exist")
                                {
                                    MappingState::Pending => {
                                        let external_multiaddr =
                                            mapping.external_addr(gateway.external_addr);
                                        self.pending_events.push_back(Event::NewExternalAddr(
                                            external_multiaddr.clone(),
                                        ));
                                        tracing::debug!(
                                            address=%mapping.internal_addr,
                                            protocol=%mapping.protocol,
                                            "successfully mapped UPnP for protocol"
                                        );
                                        return Poll::Ready(ToSwarm::ExternalAddrConfirmed(
                                            external_multiaddr,
                                        ));
                                    }
                                    MappingState::Active(_) => {
                                        tracing::debug!(
                                            address=%mapping.internal_addr,
                                            protocol=%mapping.protocol,
                                            "successfully renewed UPnP mapping for protocol"
                                        );
                                    }
                                    _ => unreachable!(),
                                }
                            }
                            GatewayEvent::MapFailure(mapping, err) => {
                                match self
                                    .mappings
                                    .insert(mapping.clone(), MappingState::Failed)
                                    .expect("mapping should exist")
                                {
                                    MappingState::Active(_) => {
                                        tracing::debug!(
                                            address=%mapping.internal_addr,
                                            protocol=%mapping.protocol,
                                            "failed to remap UPnP mapped for protocol: {err}"
                                        );
                                        let external_multiaddr =
                                            mapping.external_addr(gateway.external_addr);
                                        self.pending_events.push_back(Event::ExpiredExternalAddr(
                                            external_multiaddr.clone(),
                                        ));
                                        return Poll::Ready(ToSwarm::ExternalAddrExpired(
                                            external_multiaddr,
                                        ));
                                    }
                                    MappingState::Pending => {
                                        tracing::debug!(
                                            address=%mapping.internal_addr,
                                            protocol=%mapping.protocol,
                                            "failed to map UPnP mapped for protocol: {err}"
                                        );
                                    }
                                    _ => {
                                        unreachable!()
                                    }
                                }
                            }
                            GatewayEvent::Removed(mapping) => {
                                tracing::debug!(
                                    address=%mapping.internal_addr,
                                    protocol=%mapping.protocol,
                                    "successfully removed UPnP mapping for protocol"
                                );
                                self.mappings
                                    .remove(&mapping)
                                    .expect("mapping should exist");
                            }
                            GatewayEvent::RemovalFailure(mapping, err) => {
                                tracing::debug!(
                                    address=%mapping.internal_addr,
                                    protocol=%mapping.protocol,
                                    "could not remove UPnP mapping for protocol: {err}"
                                );
                                if let Err(err) = gateway
                                    .sender
                                    .try_send(GatewayRequest::RemoveMapping(mapping.clone()))
                                {
                                    tracing::debug!(
                                        multiaddress=%mapping.multiaddr,
                                        "could not request port removal for multiaddress on the gateway: {}",
                                        err
                                    );
                                }
                            }
                        }
                    }

                    // Renew expired and request inactive mappings.
                    self.mappings.renew(gateway, cx);
                    return Poll::Pending;
                }
                _ => return Poll::Pending,
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
