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
    collections::VecDeque,
    error::Error,
    net::IpAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use crate::{
    mapping_list::{Mapping, MappingList, MappingState},
    tokio::{is_addr_global, Gateway},
};
use futures::{channel::oneshot, Future, StreamExt};
use futures_timer::Delay;
use libp2p_core::{Endpoint, Multiaddr};
use libp2p_swarm::{
    derive_prelude::PeerId, dummy, ConnectionDenied, ConnectionId, ExpiredListenAddr, FromSwarm,
    NetworkBehaviour, NewListenAddr, ToSwarm,
};
use tracing::debug;

/// The default duration in seconds of a port mapping on the gateway.
const MAPPING_DURATION: u64 = 3600;

/// Interval to wait before performing a new mapping attempt.
const MAPPING_RETRY_INTERVAL: u64 = 900;

/// Default interval to wait before performing a new gateway discovery.
const GW_DISCOVERY_INTERVAL: u64 = 3600;

#[derive(Debug, Clone)]
pub struct Config {
    /// The mapping description.
    pub(crate) mapping_description: String,

    /// The duration of the port mapping.
    /// Renew will occur at half this duration
    pub(crate) mapping_duration: Duration,

    /// In case of failure, the duration between 2 mapping attempts
    pub(crate) mapping_retry_interval: Duration,

    /// The duration between gateway discovery.
    pub(crate) gw_discovery_interval: Duration,
}

impl Config {
    /// Creates a new [`Config`] with the following default settings:
    ///
    ///   * [`Config::mapping_description`] "libp2p-mapping"
    ///   * [`Config::mapping_duration`] 1h
    ///   * [`Config::mapping_retry_interval`] 30mn
    ///   * [`Config::gw_discovery_interval`] 1h
    ///
    pub fn new() -> Self {
        Self {
            mapping_description: "rust-libp2p mapping".to_owned(),
            mapping_duration: Duration::from_secs(MAPPING_DURATION),
            mapping_retry_interval: Duration::from_secs(MAPPING_RETRY_INTERVAL),
            gw_discovery_interval: Duration::from_secs(GW_DISCOVERY_INTERVAL),
        }
    }

    /// Sets the mapping description
    pub fn with_mapping_description(mut self, description: impl Into<String>) -> Self {
        self.mapping_description = description.into();
        self
    }

    /// Sets the mapping duration.
    pub fn with_mapping_duration(mut self, duration: Duration) -> Self {
        self.mapping_duration = duration;
        self
    }

    /// Sets the mapping retry interval.
    pub fn with_mapping_retry_interval(mut self, interval: Duration) -> Self {
        self.mapping_retry_interval = interval;
        self
    }

    /// Sets the gateway discovery interval.
    pub fn with_gw_discovery_interval(mut self, interval: Duration) -> Self {
        self.gw_discovery_interval = interval;
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`Gateway`] Request.
#[derive(Debug)]
pub(crate) enum GatewayRequest {
    AddMapping {
        mapping: Mapping,
        duration: Duration,
    },
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

/// Current state of the UPnP [`Gateway`].
enum GatewayState {
    Searching(oneshot::Receiver<Result<Gateway, Box<dyn std::error::Error + Send + Sync>>>),
    Available(Gateway),
    GatewayNotFound(Delay),
    NonRoutableGateway(IpAddr),
}

/// The event produced by `Behaviour`.
#[derive(Debug)]
pub enum Event {
    /// The IGD gateway was not found.
    GatewayNotFound,
    /// The Gateway is not exposed directly to the public network.
    NonRoutableGateway,
}

type GatewaySearchFunctionResult = oneshot::Receiver<Result<Gateway, Box<dyn Error + Send + Sync>>>;
type GatewaySearchFunction = fn(String) -> GatewaySearchFunctionResult;

/// A [`NetworkBehaviour`] for UPnP port mapping. Automatically tries to map the external port
/// to an internal address on the gateway on a [`FromSwarm::NewListenAddr`].
pub struct Behaviour {
    /// Configuration
    config: Config,

    /// UPnP interface state.
    state: GatewayState,

    /// List of port mappings.
    mappings: MappingList,

    /// Pending behaviour events to be emitted.
    pending_events: VecDeque<Event>,

    gateway_search_function: GatewaySearchFunction,
}

impl Behaviour {
    pub fn new(config: Config) -> Self {
        Self::new_with_gateway_search_function(config, crate::tokio::search_gateway)
    }

    fn new_with_gateway_search_function(
        config: Config,
        gateway_search_function: GatewaySearchFunction,
    ) -> Self {
        let description = config.mapping_description.clone();
        Self {
            config,
            state: GatewayState::Searching(gateway_search_function(description)),
            mappings: Default::default(),
            pending_events: VecDeque::new(),
            gateway_search_function,
        }
    }
}

impl Default for Behaviour {
    fn default() -> Self {
        Self::new(Config::new())
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
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::NewListenAddr(NewListenAddr {
                listener_id: _,
                addr: multiaddr,
            }) => {
                let Ok(mapping) = Mapping::try_from(multiaddr) else {
                    return;
                };

                if let Some((existing_mapping, _state)) = self.mappings.get_key_value(&mapping) {
                    debug!(
                        %multiaddr,
                        mapped_multiaddress=%existing_mapping.multiaddr,
                        "multiaddr is already being mapped"
                    );
                    return;
                }

                match &mut self.state {
                    GatewayState::Searching(_) => {
                        // As the gateway is not yet available we add the mapping with `MappingState::Inactive`
                        // so that when and if it becomes available we map it.
                        self.mappings.insert(mapping, MappingState::Inactive);
                    }
                    GatewayState::Available(ref mut gateway) => {
                        match gateway.sender.try_send(GatewayRequest::AddMapping {
                            mapping: mapping.clone(),
                            duration: self.config.mapping_duration,
                        }) {
                            Ok(_) => self.mappings.insert(mapping, MappingState::Pending),
                            Err(error) => {
                                debug!(
                                    multiaddr=%mapping.multiaddr,
                                    "could not request port mapping for multiaddr on the gateway: {error:?}",
                                );
                                self.mappings.insert(mapping, MappingState::Inactive)
                            }
                        };
                    }
                    GatewayState::GatewayNotFound(_) => {
                        debug!(
                            %multiaddr,
                            "network gateway not found, UPnP port mapping of multiaddr discarded"
                        );
                    }
                    GatewayState::NonRoutableGateway(addr) => {
                        debug!(
                            %multiaddr,
                            network_gateway_ip=%addr,
                            "the network gateway is not exposed to the public network. UPnP port mapping of multiaddr discarded"
                        );
                    }
                };
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
                listener_id: _,
                addr,
            }) => {
                let Ok(mapping) = Mapping::try_from(addr) else {
                    return;
                };

                if let Some((mapping, _state)) = self.mappings.remove_entry(&mapping) {
                    if let GatewayState::Available(ref mut gateway) = &mut self.state {
                        if let Err(err) = gateway
                            .sender
                            .try_send(GatewayRequest::RemoveMapping(mapping.clone()))
                        {
                            debug!(
                                multiaddr=%mapping.multiaddr,
                                "could not request port removal for multiaddr on the gateway: {}",
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
        void::unreachable(event)
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
            match &mut self.state {
                GatewayState::Searching(fut) => match Pin::new(fut).poll(cx) {
                    Poll::Ready(result) => {
                        match result.expect("sender shouldn't have been dropped") {
                            Ok(gw) => {
                                if !is_addr_global(gw.external_addr) {
                                    self.state = GatewayState::NonRoutableGateway(gw.external_addr);
                                    debug!(gw_addr = %gw.internal_addr, external_gw_addr = %gw.external_addr, "Found unroutable gateway");
                                    return Poll::Ready(ToSwarm::GenerateEvent(
                                        Event::NonRoutableGateway,
                                    ));
                                }
                                debug!(gw_addr = %gw.internal_addr, external_gw_addr = %gw.external_addr, "Found gateway");
                                self.state = GatewayState::Available(gw);
                            }
                            Err(error) => {
                                debug!(
                                    ?error,
                                    "Failed to find gateway - Scheduling next gateway search in {}s",
                                    self.config.gw_discovery_interval.as_secs()
                                );
                                self.state = GatewayState::GatewayNotFound(Delay::new(
                                    self.config.gw_discovery_interval,
                                ));
                                return Poll::Ready(ToSwarm::GenerateEvent(Event::GatewayNotFound));
                            }
                        }
                    }
                    Poll::Pending => return Poll::Pending,
                },
                GatewayState::Available(gateway) => {
                    // Poll pending mapping requests.
                    let gateway_receiver_poll_result = gateway.receiver.poll_next_unpin(cx);
                    if let Poll::Ready(Some(event)) = gateway_receiver_poll_result {
                        if let Some(to_swarm) = self.mappings.handle_gateway_event(
                            event,
                            &self.config,
                            gateway.external_addr,
                        ) {
                            return Poll::Ready(to_swarm);
                        }
                        continue;
                    }

                    // Renew expired and request inactive mappings.
                    for mapping in self.mappings.renew(cx) {
                        if let Err(err) = gateway.sender.try_send(GatewayRequest::AddMapping {
                            mapping: mapping.clone(),
                            duration: self.config.mapping_duration,
                        }) {
                            debug!(
                                multiaddr=%mapping.multiaddr,
                                "could not request port mapping for multiaddr on the gateway: {}",
                                err
                            );
                        }
                    }
                    return Poll::Pending;
                }
                GatewayState::GatewayNotFound(timer) => match Pin::new(timer).poll(cx) {
                    Poll::Ready(_) => {
                        self.state = GatewayState::Searching((self.gateway_search_function)(
                            self.config.mapping_description.clone(),
                        ));
                    }
                    Poll::Pending => return Poll::Pending,
                },
                _ => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use super::*;

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn being_called_back_after_failed_gateway_discovery() {
        const FLAKES_AVOIDANCE_MS: u64 = 10;
        const SEARCH_DELAY_MS: u64 = 10;
        const GW_DISCOVERY_INTERVAL_MS: u64 = 2000;

        static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);
        fn search_function(_description: String) -> GatewaySearchFunctionResult {
            let (search_result_sender, search_result_receiver) = oneshot::channel();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(SEARCH_DELAY_MS)).await;
                CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                search_result_sender.send(Err("Failed".into())).unwrap();
            });

            search_result_receiver
        }
        assert_eq!(0, CALL_COUNT.load(Ordering::SeqCst));

        let config = Config::new()
            .with_gw_discovery_interval(Duration::from_millis(GW_DISCOVERY_INTERVAL_MS));
        let mut behaviour = Behaviour::new_with_gateway_search_function(config, search_function);

        match tokio::time::timeout(
            Duration::from_millis(SEARCH_DELAY_MS + FLAKES_AVOIDANCE_MS),
            std::future::poll_fn(|cx| behaviour.poll(cx)),
        )
        .await
        {
            Ok(to_swarm) => assert!(matches!(
                to_swarm,
                ToSwarm::GenerateEvent(Event::GatewayNotFound)
            )),
            Err(_) => assert!(false, "Search function should be invoked in less than 10ms"),
        };
        assert_eq!(1, CALL_COUNT.load(Ordering::SeqCst));

        match tokio::time::timeout(
            Duration::from_millis(GW_DISCOVERY_INTERVAL_MS + SEARCH_DELAY_MS + SEARCH_DELAY_MS),
            std::future::poll_fn(|cx| behaviour.poll(cx)),
        )
        .await
        {
            Ok(to_swarm) => assert!(matches!(
                to_swarm,
                ToSwarm::GenerateEvent(Event::GatewayNotFound)
            )),
            Err(_) => assert!(
                false,
                "Search function should be called back in less than 2s"
            ),
        };
        assert_eq!(2, CALL_COUNT.load(Ordering::SeqCst));
    }
}
