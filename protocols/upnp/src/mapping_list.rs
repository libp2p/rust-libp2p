use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    net::{IpAddr, SocketAddr, SocketAddrV4},
    ops::{Deref, DerefMut},
    pin::Pin,
    task::Context,
};

use crate::behaviour::GatewayEvent;
use futures::Future;
use futures_timer::Delay;
use igd_next::PortMappingProtocol;
use libp2p_core::{multiaddr, Multiaddr};
use libp2p_swarm::ToSwarm;
use tracing::debug;
use void::Void;

use crate::behaviour::{Config, Event};

/// Mapping of a Protocol and Port on the gateway.
#[derive(Debug, Clone)]
pub(crate) struct Mapping {
    pub(crate) protocol: PortMappingProtocol,
    pub(crate) internal_addr: SocketAddr,
    pub(crate) multiaddr: Multiaddr,
}

impl Mapping {
    /// Given the input gateway address, calculate the
    /// open external `Multiaddr`.
    fn external_addr(&self, gateway_addr: IpAddr) -> Multiaddr {
        let addr = multiaddr::Protocol::from(gateway_addr);
        self.multiaddr
            .replace(0, |_| Some(addr))
            .expect("multiaddr should be valid")
    }
}

impl Hash for Mapping {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(&self.protocol).hash(state);
        self.internal_addr.hash(state);
    }
}

impl Eq for Mapping {}
impl PartialEq for Mapping {
    fn eq(&self, other: &Self) -> bool {
        self.protocol == other.protocol && self.internal_addr == other.internal_addr
    }
}

impl TryFrom<&Multiaddr> for Mapping {
    type Error = ();

    fn try_from(multiaddr: &Multiaddr) -> Result<Self, Self::Error> {
        let (internal_addr, protocol) = match multiaddr_to_socketaddr_protocol(multiaddr) {
            Ok(addr_port) => addr_port,
            Err(()) => {
                debug!("multiaddr not supported for UPnP {multiaddr}");
                return Err(());
            }
        };

        Ok(Mapping {
            protocol,
            internal_addr,
            multiaddr: multiaddr.clone(),
        })
    }
}

/// Current state of a [`Mapping`].
#[derive(Debug)]
pub(crate) enum MappingState {
    /// Port mapping is inactive, will be requested or re-requested on the next iteration.
    Inactive,
    /// Port mapping/removal has been requested on the gateway.
    Pending,
    /// Port mapping is active with the inner timeout or none if renewal is already in progress.
    Active(Option<Delay>),
    /// Port mapping failed, we will try again.
    Failed(Delay),
}

/// A list of port mappings and its state.
#[derive(Debug, Default)]
pub(crate) struct MappingList(HashMap<Mapping, MappingState>);

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
    pub(crate) fn renew(&mut self, cx: &mut Context<'_>) -> Vec<Mapping> {
        let mut mappings_to_renew = Vec::new();

        for (mapping, state) in self.iter_mut() {
            let request_mapping = match state {
                MappingState::Inactive => {
                    *state = MappingState::Pending;
                    true
                }
                MappingState::Failed(timer) => {
                    let ready_to_renew = Pin::new(timer).poll(cx).is_ready();
                    if ready_to_renew {
                        *state = MappingState::Pending;
                    }
                    ready_to_renew
                }
                MappingState::Active(Some(timeout)) => {
                    let ready_to_renew = Pin::new(timeout).poll(cx).is_ready();
                    if ready_to_renew {
                        *state = MappingState::Active(None);
                    }
                    ready_to_renew
                }
                MappingState::Active(None) | MappingState::Pending => false,
            };
            if request_mapping {
                mappings_to_renew.push(mapping.clone());
            }
        }

        mappings_to_renew
    }

    pub(crate) fn handle_gateway_event(
        &mut self,
        event: GatewayEvent,
        config: &Config,
        gateway_external_addr: IpAddr,
    ) -> Option<ToSwarm<Event, Void>> {
        match event {
            GatewayEvent::Mapped(mapping) => {
                if let Some(MappingState::Pending) = self.insert(
                    mapping.clone(),
                    MappingState::Active(Some(Delay::new(config.mapping_duration / 2))),
                ) {
                    debug!(?mapping, "UPnP mapping activated successfully");
                    let external_multiaddr = mapping.external_addr(gateway_external_addr);
                    return Some(ToSwarm::ExternalAddrConfirmed(external_multiaddr));
                } else {
                    debug!(?mapping, "UPnP mapping renewed successfully");
                }
            }
            GatewayEvent::MapFailure(mapping, err) => {
                if let Some(MappingState::Active(_)) = self.insert(
                    mapping.clone(),
                    MappingState::Failed(Delay::new(config.mapping_retry_interval)),
                ) {
                    debug!(?mapping, "UPnP mapping failed to be renewed: {err}");
                    let external_multiaddr = mapping.external_addr(gateway_external_addr);
                    return Some(ToSwarm::ExternalAddrExpired(external_multiaddr));
                } else {
                    debug!(?mapping, "UPnP mapping failed to be activated: {err}");
                }
            }
            GatewayEvent::Removed(mapping) => {
                debug!(?mapping, "UPnP mapping removed successfully");
                self.remove(&mapping);
            }
            GatewayEvent::RemovalFailure(mapping, err) => {
                debug!(?mapping, "UPnP mapping failed to be removed: {err}");
                // Do not trigger a removal to avoid infinite loop on RemovalFailure.
                // Expiration should remove it anyway.
                self.remove(&mapping);
            }
        };

        None
    }
}

/// Extracts a [`SocketAddrV4`] and [`PortMappingProtocol`] from a given [`Multiaddr`].
///
/// Fails if the given [`Multiaddr`] does not begin with an IP
/// protocol encapsulating a TCP or UDP port.
fn multiaddr_to_socketaddr_protocol(
    addr: &Multiaddr,
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
    use std::task::Poll;
    use std::time::Duration;

    use super::*;

    const FLAKES_AVOIDANCE_MS: u64 = 10;

    fn fake_mapping_1() -> Mapping {
        Mapping {
            protocol: PortMappingProtocol::TCP,
            internal_addr: "192.168.1.2:1234".parse().unwrap(),
            multiaddr: "/ip4/192.168.1.2/tcp/1234".parse().unwrap(),
        }
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn renew_inactive() {
        let mut mapping_list = MappingList::default();
        mapping_list.insert(fake_mapping_1(), MappingState::Inactive);
        let to_renew = std::future::poll_fn(|cx| Poll::Ready(mapping_list.renew(cx))).await;
        assert_eq!(to_renew, vec![fake_mapping_1()]);
        assert!(matches!(
            mapping_list.get(&fake_mapping_1()),
            Some(MappingState::Pending)
        ));

        // don't renew while renewing is already in progress
        let to_renew = std::future::poll_fn(|cx| Poll::Ready(mapping_list.renew(cx))).await;
        assert_eq!(to_renew, vec![]);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn renew_pending() {
        let mut mapping_list = MappingList::default();
        mapping_list.insert(fake_mapping_1(), MappingState::Pending);
        let to_renew = std::future::poll_fn(|cx| Poll::Ready(mapping_list.renew(cx))).await;
        assert_eq!(to_renew, vec![]);
        assert!(matches!(
            mapping_list.get(&fake_mapping_1()),
            Some(MappingState::Pending)
        ));

        // don't renew while renewing is already in progress
        let to_renew = std::future::poll_fn(|cx| Poll::Ready(mapping_list.renew(cx))).await;
        assert_eq!(to_renew, vec![]);
    }

    async fn renew_timeout(
        duration_ms: u64,
        mapping_list: &mut MappingList,
    ) -> Result<Vec<Mapping>, tokio::time::error::Elapsed> {
        tokio::time::timeout(
            Duration::from_millis(duration_ms),
            std::future::poll_fn(|cx| {
                let to_renew = mapping_list.renew(cx);
                match to_renew.is_empty() {
                    true => Poll::Pending,
                    false => Poll::Ready(to_renew),
                }
            }),
        )
        .await
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn renew_failed() {
        const RETRY_INTERVAL_MS: u64 = 30;
        let mut mapping_list = MappingList::default();
        mapping_list.insert(
            fake_mapping_1(),
            MappingState::Failed(Delay::new(Duration::from_millis(RETRY_INTERVAL_MS))),
        );

        assert!(
            renew_timeout(RETRY_INTERVAL_MS - FLAKES_AVOIDANCE_MS, &mut mapping_list)
                .await
                .is_err(),
            "Renew should not trigger before 30ms"
        );

        match renew_timeout(RETRY_INTERVAL_MS * 2, &mut mapping_list).await {
            Ok(to_renew) => assert_eq!(to_renew, vec![fake_mapping_1()]),
            Err(_) => assert!(false, "Renew should trigger after 30ms"),
        };
        assert!(matches!(
            mapping_list.get(&fake_mapping_1()),
            Some(MappingState::Pending)
        ));

        // don't renew while renewing is already in progress
        let to_renew = std::future::poll_fn(|cx| Poll::Ready(mapping_list.renew(cx))).await;
        assert_eq!(to_renew, vec![]);
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn renew_active() {
        const RETRY_INTERVAL_MS: u64 = 30;
        let mut mapping_list = MappingList::default();
        mapping_list.insert(
            fake_mapping_1(),
            MappingState::Active(Some(Delay::new(Duration::from_millis(RETRY_INTERVAL_MS)))),
        );

        assert!(
            renew_timeout(RETRY_INTERVAL_MS - FLAKES_AVOIDANCE_MS, &mut mapping_list)
                .await
                .is_err(),
            "Renew should not trigger before 30ms"
        );

        match renew_timeout(RETRY_INTERVAL_MS * 2, &mut mapping_list).await {
            Ok(to_renew) => assert_eq!(to_renew, vec![fake_mapping_1()]),
            Err(_) => assert!(false, "Renew should trigger after 30ms"),
        };
        assert!(matches!(
            mapping_list.get(&fake_mapping_1()),
            Some(MappingState::Active(None))
        ));

        // don't renew while renewing is already in progress
        let to_renew = std::future::poll_fn(|cx| Poll::Ready(mapping_list.renew(cx))).await;
        assert_eq!(to_renew, vec![]);
    }

    #[test]
    fn handle_gateway_event_mapped() {
        let external_addr = "8.8.8.8".parse().unwrap();
        let mut mapping_list = MappingList::default();
        mapping_list.insert(fake_mapping_1(), MappingState::Pending);
        let to_swarm_opt = mapping_list.handle_gateway_event(
            GatewayEvent::Mapped(fake_mapping_1()),
            &Config::new(),
            external_addr,
        );

        match to_swarm_opt {
            Some(ToSwarm::ExternalAddrConfirmed(addr)) => {
                assert_eq!(addr, "/ip4/8.8.8.8/tcp/1234".parse().unwrap())
            }
            _ => assert!(false),
        };

        assert!(matches!(
            mapping_list.get(&fake_mapping_1()),
            Some(MappingState::Active(Some(_)))
        ));

        let to_swarm_opt = mapping_list.handle_gateway_event(
            GatewayEvent::Mapped(fake_mapping_1()),
            &Config::new(),
            external_addr,
        );

        assert!(to_swarm_opt.is_none());
    }

    #[test]
    fn handle_gateway_event_map_failure() {
        let external_addr = "8.8.8.8".parse().unwrap();
        let mut mapping_list = MappingList::default();
        mapping_list.insert(fake_mapping_1(), MappingState::Active(None));
        let to_swarm_opt = mapping_list.handle_gateway_event(
            GatewayEvent::MapFailure(
                fake_mapping_1(),
                igd_next::AddPortError::ActionNotAuthorized.into(),
            ),
            &Config::new(),
            external_addr,
        );

        match to_swarm_opt {
            Some(ToSwarm::ExternalAddrExpired(addr)) => {
                assert_eq!(addr, "/ip4/8.8.8.8/tcp/1234".parse().unwrap())
            }
            _ => assert!(false),
        };

        assert!(matches!(
            mapping_list.get(&fake_mapping_1()),
            Some(MappingState::Failed(_))
        ));

        let to_swarm_opt = mapping_list.handle_gateway_event(
            GatewayEvent::MapFailure(
                fake_mapping_1(),
                igd_next::AddPortError::ActionNotAuthorized.into(),
            ),
            &Config::new(),
            external_addr,
        );

        assert!(to_swarm_opt.is_none());
    }
}
