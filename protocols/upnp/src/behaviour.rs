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
    marker::PhantomData,
    net::SocketAddrV4,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use crate::{
    provider::{Gateway, Protocol, Provider},
    Config,
};
use futures::{future::BoxFuture, Future, FutureExt, StreamExt};
use futures_timer::Delay;
use libp2p_core::{multiaddr, transport::ListenerId, Endpoint, Multiaddr};
use libp2p_swarm::{
    derive_prelude::PeerId, dummy, ConnectionDenied, ConnectionId, ExpiredListenAddr, FromSwarm,
    NetworkBehaviour, NewListenAddr, PollParameters, ToSwarm,
};

/// The duration in seconds of a port mapping on the gateway.
const MAPPING_DURATION: u32 = 3600;

/// Renew the Mapping every half of `MAPPING_DURATION` to avoid the port being unmapped.
const MAPPING_TIMEOUT: u64 = MAPPING_DURATION as u64 / 2;

/// A [`Gateway`] Request.
#[derive(Debug)]
pub(crate) enum GatewayRequest {
    AddMapping(Mapping, Option<u32>),
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
    /// There was a failure removing the mapping port.
    RemovalFailure(Mapping, Box<dyn Error + Send + Sync + 'static>),
}

/// Mapping of a Protocol and Port on the gateway.
#[derive(Debug, Clone)]
pub(crate) struct Mapping {
    pub(crate) listener_id: ListenerId,
    pub(crate) protocol: Protocol,
    pub(crate) multiaddr: Multiaddr,
    pub(crate) internal_addr: SocketAddrV4,
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
enum MappingState {
    /// Port mapping is inactive, will be requested or re-requested on the next iteration.
    Inactive,
    /// Port mapping/removal has been requested on the gateway.
    Pending,
    /// Port mapping is active with the inner timeout.
    Active(Delay),
    /// Port mapping failed, we will try again.
    Failed,
    /// Port mapping is permanent on the Gateway.
    Permanent,
}

/// Current state of the UPnP [`Gateway`].
enum GatewayState {
    Searching(BoxFuture<'static, Result<Gateway, Box<dyn std::error::Error>>>),
    Available(Gateway),
    GatewayNotFound,
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
}

/// A `NetworkBehaviour` for UPnP port mapping. Automatically tries to map the external port
/// to an internal address on the gateway on a `FromSwarm::NewListenAddr`.
pub struct Behaviour<P>
where
    P: Provider,
{
    /// Gateway config.
    config: Config,
    /// UPnP interface state.
    state: GatewayState,

    /// List of port mappings.
    mappings: HashMap<Mapping, MappingState>,

    /// Pending behaviour events to be emitted.
    pending_events: VecDeque<Event>,

    /// Provider.
    provider: PhantomData<P>,
}

impl<P> Default for Behaviour<P>
where
    P: Provider + 'static,
{
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl<P> Behaviour<P>
where
    P: Provider + 'static,
{
    /// Builds a new `UPnP` behaviour.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: GatewayState::Searching(P::search_gateway(config).boxed()),
            mappings: Default::default(),
            pending_events: VecDeque::new(),
            provider: PhantomData,
        }
    }
}

impl<P> NetworkBehaviour for Behaviour<P>
where
    P: Provider + 'static,
{
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
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::NewListenAddr(NewListenAddr {
                listener_id,
                addr: multiaddr,
            }) => {
                let (addr, protocol) = match multiaddr_to_socketaddr_protocol(multiaddr.clone()) {
                    Ok(addr_port) => addr_port,
                    Err(_) => {
                        log::debug!("multiaddress not supported for UPnP {multiaddr}");
                        return;
                    }
                };

                if let Some((mapping, _state)) = self
                    .mappings
                    .iter()
                    .find(|(mapping, _state)| mapping.internal_addr.port() == addr.port())
                {
                    log::debug!("port from multiaddress {multiaddr} is already being mapped to another multiaddr: {}", mapping.multiaddr);
                    return;
                }

                match &mut self.state {
                    GatewayState::Searching(_) => {
                        // As the gateway is not yet available we add the mapping with `MappingState::Inactive`
                        // so that when and if it becomes available we map it.
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

                        let duration = self.config.temporary.then_some(MAPPING_DURATION);
                        if let Err(err) = gateway
                            .sender
                            .try_send(GatewayRequest::AddMapping(mapping.clone(), duration))
                        {
                            log::debug!(
                                "could not request port mapping for {} on the gateway: {}",
                                mapping.multiaddr,
                                err
                            );
                        }

                        self.mappings.insert(mapping, MappingState::Pending);
                    }
                    GatewayState::GatewayNotFound => {
                        log::debug!(
                            "network gateway not found, UPnP port mapping of {multiaddr} discarded"
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
                            log::debug!(
                                "could not request port removal for {} on the gateway: {}",
                                mapping.multiaddr,
                                err
                            );
                        }
                        self.mappings.insert(mapping, MappingState::Pending);
                    }
                }
            }
            FromSwarm::ConnectionEstablished(_)
            | FromSwarm::ConnectionClosed(_)
            | FromSwarm::AddressChange(_)
            | FromSwarm::DialFailure(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListener(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddrCandidate(_)
            | FromSwarm::ExternalAddrConfirmed(_)
            | FromSwarm::ExternalAddrExpired(_) => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>> {
        loop {
            // If there are pending addresses to be emitted we emit them first.
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(ToSwarm::GenerateEvent(event));
            }

            // We then check the `Gateway` current state.
            match self.state {
                GatewayState::Searching(ref mut fut) => match Pin::new(fut).poll(cx) {
                    Poll::Ready(result) => match result {
                        Ok(gateway) => {
                            self.state = GatewayState::Available(gateway);
                        }
                        Err(err) => {
                            log::debug!("could not find gateway: {err}");
                            self.state = GatewayState::GatewayNotFound;
                            return Poll::Ready(ToSwarm::GenerateEvent(Event::GatewayNotFound));
                        }
                    },
                    Poll::Pending => return Poll::Pending,
                },
                GatewayState::Available(ref mut gateway) => {
                    // Check pending mappings.
                    if let Poll::Ready(Some(result)) = gateway.receiver.poll_next_unpin(cx) {
                        match result {
                            GatewayEvent::Mapped(mapping) => {
                                let state = self
                                    .mappings
                                    .get_mut(&mapping)
                                    .expect("mapping should exist");
                                match state {
                                    MappingState::Pending => {
                                        log::debug!(
                                            "succcessfuly UPnP mapped {} for {} protocol",
                                            mapping.internal_addr,
                                            mapping.protocol
                                        );
                                        let external_multiaddr = mapping
                                            .multiaddr
                                            .replace(0, |_| {
                                                Some(multiaddr::Protocol::Ip4(gateway.addr))
                                            })
                                            .expect("multiaddr should be valid");
                                        *state = if self.config.temporary {
                                            MappingState::Permanent
                                        } else {
                                            MappingState::Active(Delay::new(Duration::from_secs(
                                                MAPPING_TIMEOUT,
                                            )))
                                        };

                                        self.pending_events.push_back(Event::NewExternalAddr(
                                            external_multiaddr.clone(),
                                        ));
                                        return Poll::Ready(ToSwarm::ExternalAddrConfirmed(
                                            external_multiaddr,
                                        ));
                                    }
                                    MappingState::Active(_) => {
                                        *state = MappingState::Active(Delay::new(
                                            Duration::from_secs(MAPPING_TIMEOUT),
                                        ));

                                        log::debug!(
                                            "succcessfuly remapped UPnP {} for {} protocol",
                                            mapping.internal_addr,
                                            mapping.protocol
                                        );
                                    }
                                    MappingState::Inactive
                                    | MappingState::Permanent
                                    | MappingState::Failed => {
                                        unreachable!()
                                    }
                                }
                            }
                            GatewayEvent::MapFailure(mapping, err) => {
                                let state = self
                                    .mappings
                                    .get_mut(&mapping)
                                    .expect("mapping should exist");

                                match state {
                                    MappingState::Active(_) => {
                                        log::debug!(
                                            "failed to remap UPnP mapped {} for {} protocol: {err}",
                                            mapping.internal_addr,
                                            mapping.protocol
                                        );
                                        *state = MappingState::Failed;
                                        let external_multiaddr = mapping
                                            .multiaddr
                                            .replace(0, |_| {
                                                Some(multiaddr::Protocol::Ip4(gateway.addr))
                                            })
                                            .expect("multiaddr should be valid");

                                        self.pending_events.push_back(Event::ExpiredExternalAddr(
                                            external_multiaddr.clone(),
                                        ));
                                        return Poll::Ready(ToSwarm::ExternalAddrExpired(
                                            external_multiaddr,
                                        ));
                                    }
                                    MappingState::Pending => {
                                        log::debug!(
                                            "failed to map upnp mapped {} for {} protocol: {err}",
                                            mapping.internal_addr,
                                            mapping.protocol
                                        );
                                        *state = MappingState::Failed;
                                    }
                                    MappingState::Inactive
                                    | MappingState::Permanent
                                    | MappingState::Failed => {
                                        unreachable!()
                                    }
                                }
                            }
                            GatewayEvent::Removed(mapping) => {
                                log::debug!(
                                    "succcessfuly removed UPnP mapping {} for {} protocol",
                                    mapping.internal_addr,
                                    mapping.protocol
                                );
                                self.mappings
                                    .remove(&mapping)
                                    .expect("mapping should exist");
                            }
                            GatewayEvent::RemovalFailure(mapping, err) => {
                                log::debug!(
                                    "could not remove UPnP mapping {} for {} protocol: {err}",
                                    mapping.internal_addr,
                                    mapping.protocol
                                );
                                if let Err(err) = gateway
                                    .sender
                                    .try_send(GatewayRequest::RemoveMapping(mapping.clone()))
                                {
                                    log::debug!(
                                        "could not request port removal for {} on the gateway: {}",
                                        mapping.multiaddr,
                                        err
                                    );
                                }
                            }
                        }
                    }

                    // Renew expired and request inactive mappings.
                    for (mapping, state) in self.mappings.iter_mut() {
                        match state {
                            MappingState::Inactive | MappingState::Failed => {
                                let duration = self.config.temporary.then_some(MAPPING_DURATION);
                                if let Err(err) = gateway
                                    .sender
                                    .try_send(GatewayRequest::AddMapping(mapping.clone(), duration))
                                {
                                    log::debug!(
                                        "could not request port mapping for {} on the gateway: {}",
                                        mapping.multiaddr,
                                        err
                                    );
                                }
                                *state = MappingState::Pending;
                            }
                            MappingState::Active(timeout) => {
                                if Pin::new(timeout).poll(cx).is_ready() {
                                    let duration =
                                        self.config.temporary.then_some(MAPPING_DURATION);
                                    if let Err(err) = gateway.sender.try_send(
                                        GatewayRequest::AddMapping(mapping.clone(), duration),
                                    ) {
                                        log::debug!(
                                        "could not request port mapping for {} on the gateway: {}",
                                        mapping.multiaddr,
                                        err
                                    );
                                    }
                                }
                            }
                            MappingState::Pending | MappingState::Permanent => {}
                        }
                    }
                    return Poll::Pending;
                }
                GatewayState::GatewayNotFound => {
                    return Poll::Ready(ToSwarm::GenerateEvent(Event::GatewayNotFound));
                }
            }
        }
    }
}

/// Extracts a `SocketAddr` and `Protocol` from a given `Multiaddr`.
///
/// Fails if the given `Multiaddr` does not begin with an IP
/// protocol encapsulating a TCP or UDP port.
fn multiaddr_to_socketaddr_protocol(mut addr: Multiaddr) -> Result<(SocketAddrV4, Protocol), ()> {
    let mut port = None;
    let mut protocol = None;
    while let Some(proto) = addr.pop() {
        match proto {
            multiaddr::Protocol::Ip6(_) => {
                // Idg only supports Ipv4.
                return Err(());
            }
            multiaddr::Protocol::Ip4(ipv4) if ipv4.is_private() => match (port, protocol) {
                (Some(port), Some(protocol)) => {
                    return Ok((SocketAddrV4::new(ipv4, port), protocol));
                }
                _ => return Err(()),
            },
            multiaddr::Protocol::Tcp(portnum) => match (port, protocol) {
                (None, None) => {
                    port = Some(portnum);
                    protocol = Some(Protocol::Tcp);
                }
                _ => return Err(()),
            },
            multiaddr::Protocol::Udp(portnum) => match (port, protocol) {
                (None, None) => {
                    port = Some(portnum);
                    protocol = Some(Protocol::Udp);
                }
                _ => return Err(()),
            },

            _ => {}
        }
    }
    Err(())
}
