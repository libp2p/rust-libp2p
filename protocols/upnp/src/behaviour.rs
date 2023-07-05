// Copyright 2017 Parity Technologies (UK) Ltd.
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
    collections::HashMap,
    error::Error,
    hash::{Hash, Hasher},
    net::{Ipv4Addr, SocketAddrV4},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use crate::{
    provider::{Protocol, Provider},
    Config,
};
use futures::{future::BoxFuture, stream::FuturesUnordered, Future, FutureExt, StreamExt};
use futures_timer::Delay;
use libp2p_core::{multiaddr, transport::ListenerId, Endpoint, Multiaddr};
use libp2p_swarm::{
    derive_prelude::PeerId, dummy, ConnectionDenied, ConnectionId, ExpiredListenAddr, FromSwarm,
    NetworkBehaviour, NewListenAddr, PollParameters, ToSwarm,
};

/// The duration in seconds of a port mapping on the gateway.
const MAPPING_DURATION: u64 = 3600;

/// Renew the Mapping every half of `MAPPING_DURATION` to avoid the port being unmapped.
const MAPPING_TIMEOUT: u64 = MAPPING_DURATION / 2;

/// Map a port on the gateway.
async fn map_port<P: Provider + 'static>(
    gateway: Arc<P::Gateway>,
    mapping: Mapping,
    permanent: bool,
) -> Event {
    let duration = if permanent { 0 } else { MAPPING_DURATION };

    match P::add_port(
        gateway,
        mapping.protocol,
        mapping.internal_addr,
        Duration::from_secs(duration),
    )
    .await
    {
        Ok(()) => Event::Mapped(mapping),
        Err(err) => Event::MapFailure(mapping, err),
    }
}

/// Remove a port mapping on the gateway.
async fn remove_port_mapping<P: Provider + 'static>(
    gateway: Arc<P::Gateway>,
    mapping: Mapping,
) -> Event {
    match P::remove_port(gateway, mapping.protocol, mapping.internal_addr.port()).await {
        Ok(()) => Event::Removed(mapping),
        Err(err) => Event::RemovalFailure(mapping, err),
    }
}

/// A [`Provider::Gateway`] event.
#[derive(Debug)]
enum Event {
    /// Port was successfully mapped.
    Mapped(Mapping),
    /// There was a failure mapping port.
    MapFailure(Mapping, Box<dyn Error>),
    /// Port was successfully removed.
    Removed(Mapping),
    /// There was a failure removing the mapping port.
    RemovalFailure(Mapping, Box<dyn Error>),
}

/// Mapping of a Protocol and Port on the gateway.
#[derive(Debug, Clone)]
struct Mapping {
    listener_id: ListenerId,
    protocol: Protocol,
    multiaddr: Multiaddr,
    internal_addr: SocketAddrV4,
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
    /// Port mapping is permanent on the Gateway.
    Permanent,
}

/// Current state of the UPnP [`Provider::Gateway`].
enum GatewayState<P: Provider> {
    Searching(BoxFuture<'static, Result<(P::Gateway, Ipv4Addr), Box<dyn std::error::Error>>>),
    Available((Arc<P::Gateway>, Ipv4Addr)),
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
    state: GatewayState<P>,

    /// List of port mappings.
    mappings: HashMap<Mapping, MappingState>,

    /// Pending gateway events.
    pending_events: FuturesUnordered<BoxFuture<'static, Event>>,
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
            pending_events: Default::default(),
        }
    }
}

impl<P> NetworkBehaviour for Behaviour<P>
where
    P: Provider + 'static,
{
    type ConnectionHandler = dummy::ConnectionHandler;

    type ToSwarm = ();

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

                match &self.state {
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
                    GatewayState::Available((gateway, _external_addr)) => {
                        let mapping = Mapping {
                            listener_id,
                            protocol,
                            internal_addr: addr,
                            multiaddr: multiaddr.clone(),
                        };

                        self.pending_events.push(
                            map_port::<P>(gateway.clone(), mapping.clone(), self.config.permanent)
                                .boxed(),
                        );

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
                if let GatewayState::Available((gateway, _external_addr)) = &self.state {
                    if let Some((mapping, _state)) = self.mappings.remove_entry(&listener_id) {
                        self.pending_events.push(
                            remove_port_mapping::<P>(gateway.clone(), mapping.clone()).boxed(),
                        );
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
            match self.state {
                GatewayState::Searching(ref mut fut) => match Pin::new(fut).poll(cx) {
                    Poll::Ready(result) => match result {
                        Ok((gateway, external_addr)) => {
                            self.state =
                                GatewayState::Available((Arc::new(gateway), external_addr));
                        }
                        Err(err) => {
                            log::debug!("could not find gateway: {err}");
                            self.state = GatewayState::GatewayNotFound;
                            return Poll::Ready(ToSwarm::GenerateEvent(()));
                        }
                    },
                    Poll::Pending => return Poll::Pending,
                },
                GatewayState::Available((ref gateway, external_addr)) => {
                    // Check pending mappings.
                    if let Poll::Ready(Some(result)) = self.pending_events.poll_next_unpin(cx) {
                        match result {
                            Event::Mapped(mapping) => {
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
                                                Some(multiaddr::Protocol::Ip4(external_addr))
                                            })
                                            .expect("multiaddr should be valid");
                                        *state = if self.config.permanent {
                                            MappingState::Permanent
                                        } else {
                                            MappingState::Active(Delay::new(Duration::from_secs(
                                                MAPPING_TIMEOUT,
                                            )))
                                        };

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
                                    MappingState::Inactive | MappingState::Permanent => {
                                        unreachable!()
                                    }
                                }
                            }
                            Event::MapFailure(mapping, err) => {
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
                                        *state = MappingState::Inactive;
                                        let external_multiaddr = mapping
                                            .multiaddr
                                            .replace(0, |_| {
                                                Some(multiaddr::Protocol::Ip4(external_addr))
                                            })
                                            .expect("multiaddr should be valid");

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
                                        *state = MappingState::Inactive;
                                    }
                                    MappingState::Inactive | MappingState::Permanent => {
                                        unreachable!()
                                    }
                                }
                            }
                            Event::Removed(mapping) => {
                                log::debug!(
                                    "succcessfuly removed UPnP mapping {} for {} protocol",
                                    mapping.internal_addr,
                                    mapping.protocol
                                );
                                self.mappings
                                    .remove(&mapping)
                                    .expect("mapping should exist");
                            }
                            Event::RemovalFailure(mapping, err) => {
                                log::debug!(
                                    "could not remove UPnP mapping {} for {} protocol: {err}",
                                    mapping.internal_addr,
                                    mapping.protocol
                                );
                                self.pending_events.push(
                                    remove_port_mapping::<P>(gateway.clone(), mapping).boxed(),
                                );
                            }
                        }
                    }

                    // Renew expired and request inactive mappings.
                    for (mapping, state) in self.mappings.iter_mut() {
                        match state {
                            MappingState::Inactive => {
                                self.pending_events.push(
                                    map_port::<P>(
                                        gateway.clone(),
                                        mapping.clone(),
                                        self.config.permanent,
                                    )
                                    .boxed(),
                                );
                                *state = MappingState::Pending;
                            }
                            MappingState::Active(timeout) => {
                                if Pin::new(timeout).poll(cx).is_ready() {
                                    self.pending_events.push(
                                        map_port::<P>(gateway.clone(), mapping.clone(), false)
                                            .boxed(),
                                    );
                                }
                            }
                            MappingState::Pending | MappingState::Permanent => {}
                        }
                    }
                    return Poll::Pending;
                }
                GatewayState::GatewayNotFound => {
                    return Poll::Ready(ToSwarm::GenerateEvent(()));
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
