use std::{
    collections::{HashSet, VecDeque},
    net::IpAddr,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::{FutureExt, channel::oneshot};
use futures_timer::Delay;
use libp2p_core::{Endpoint, Multiaddr, multiaddr::Protocol, transport::PortUse};
use libp2p_identity::Keypair;
use libp2p_swarm::{
    ConnectionDenied, ConnectionId, ExpiredListenAddr, FromSwarm, NetworkBehaviour, NewListenAddr,
    THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    behaviour::{ExternalAddrConfirmed, ExternalAddrExpired},
    dummy,
};

use crate::{
    AutoTlsCertResolver,
    acme::{self, AcmeConfig, ObtainedCertificate},
    cert, encoding,
    storage::CertStore,
};

/// Renew the certificate this long before it expires.
const RENEWAL_LEAD: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Back off this long after a failed issuance before retrying.
const FAILURE_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// Events emitted by the AutoTLS [`Behaviour`].
#[derive(Debug)]
pub enum Event {
    /// A certificate was obtained (or renewed) and installed.
    CertificateObtained {
        /// The Unix timestamp (in seconds) at which the certificate expires.
        not_after_unix: i64,
    },
    /// A certificate issuance attempt failed; it will be retried after a backoff.
    IssuanceFailed(acme::Error),
}

enum State {
    /// Waiting for a public address before attempting issuance.
    Idle,
    /// An issuance task is running.
    Issuing(oneshot::Receiver<Result<ObtainedCertificate, acme::Error>>),
    /// A certificate is installed; the timer fires when it is time to renew.
    Active(Delay),
    /// Issuance failed; the timer fires when it is time to retry.
    Backoff(Delay),
}

enum AddrUpdate {
    Confirmed(Multiaddr),
    Expired(Multiaddr),
}

pub struct Behaviour<S> {
    config: AcmeConfig,
    identity: Keypair,
    peer_id_label: String,
    store: Arc<S>,
    resolver: AutoTlsCertResolver,

    /// Confirmed public transport addresses, sent to the broker for reachability verification.
    confirmed: HashSet<Multiaddr>,
    /// TCP ports of the node's `/tls/ws` listen addresses.
    wss_ports: HashSet<u16>,
    /// The `/dns4/…/tls/ws` addresses currently advertised.
    advertised: HashSet<Multiaddr>,

    state: State,
    pending_events: VecDeque<Event>,
    pending_addrs: VecDeque<AddrUpdate>,
}

impl<S> Behaviour<S>
where
    S: CertStore + 'static,
{
    pub async fn new(identity: &Keypair, config: AcmeConfig, store: S) -> Self {
        if config.install_crypto_provider {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }

        let peer_id_label = encoding::peer_id_label(identity.public().to_peer_id());
        let resolver = AutoTlsCertResolver::new();

        let mut state = State::Idle;
        if let Ok(Some(stored)) = store.load_certificate().await
            && let Ok(not_after) = cert::not_after_unix(&stored.chain_pem)
            && resolver.set_pem(&stored.chain_pem, &stored.key_pem).is_ok()
        {
            state = State::Active(renewal_delay(not_after));
        }

        Self {
            config,
            identity: identity.clone(),
            peer_id_label,
            store: Arc::new(store),
            resolver,
            confirmed: HashSet::new(),
            wss_ports: HashSet::new(),
            advertised: HashSet::new(),
            state,
            pending_events: VecDeque::new(),
            pending_addrs: VecDeque::new(),
        }
    }

    /// The certificate resolver to hand to a TLS-serving transport.
    pub fn certificate_resolver(&self) -> Arc<dyn rustls::server::ResolvesServerCert> {
        Arc::new(self.resolver.clone())
    }

    fn start_issuance(&mut self) {
        let config = self.config.clone();
        let identity = self.identity.clone();
        let store = self.store.clone();
        let addresses: Vec<String> = self.confirmed.iter().map(Multiaddr::to_string).collect();
        tracing::debug!(addresses = ?addresses, "starting AutoTLS certificate issuance");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let result =
                acme::obtain_certificate(&config, &identity, &addresses, store.as_ref()).await;
            let _ = sender.send(result);
        });
        self.state = State::Issuing(receiver);
    }

    /// Queue advertisement updates so the set of advertised addresses matches the
    /// current public IPs and `/tls/ws` ports. Only advertises while a certificate is installed.
    fn reconcile_advertised(&mut self) {
        let desired = if self.resolver.is_set() {
            self.desired_addresses()
        } else {
            HashSet::new()
        };

        for addr in desired
            .difference(&self.advertised)
            .cloned()
            .collect::<Vec<_>>()
        {
            self.advertised.insert(addr.clone());
            self.pending_addrs.push_back(AddrUpdate::Confirmed(addr));
        }
        for addr in self
            .advertised
            .difference(&desired)
            .cloned()
            .collect::<Vec<_>>()
        {
            self.advertised.remove(&addr);
            self.pending_addrs.push_back(AddrUpdate::Expired(addr));
        }
    }

    fn desired_addresses(&self) -> HashSet<Multiaddr> {
        let mut addresses = HashSet::new();
        for ip in self.confirmed.iter().filter_map(extract_ip) {
            for &port in &self.wss_ports {
                addresses.insert(forge_wss_addr(
                    ip,
                    port,
                    &self.peer_id_label,
                    &self.config.forge_domain,
                ));
            }
        }
        addresses
    }

    fn can_issue(&self) -> bool {
        !self.confirmed.is_empty() && !self.wss_ports.is_empty()
    }
}

impl<S> NetworkBehaviour for Behaviour<S>
where
    S: CertStore + 'static,
{
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: libp2p_identity::PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: libp2p_identity::PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ExternalAddrConfirmed(ExternalAddrConfirmed { addr })
                if extract_ip(addr).is_some() =>
            {
                self.confirmed.insert(addr.clone());
                self.reconcile_advertised();
            }
            FromSwarm::ExternalAddrExpired(ExternalAddrExpired { addr })
                if self.confirmed.remove(addr) =>
            {
                self.reconcile_advertised();
            }
            FromSwarm::NewListenAddr(NewListenAddr { addr, .. }) => {
                if let Some(port) = wss_port(addr) {
                    self.wss_ports.insert(port);
                    self.reconcile_advertised();
                }
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr { addr, .. }) => {
                if let Some(port) = wss_port(addr)
                    && self.wss_ports.remove(&port)
                {
                    self.reconcile_advertised();
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _: libp2p_identity::PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        libp2p_core::util::unreachable(event)
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ToSwarm<Event, THandlerInEvent<Self>>> {
        if let Some(update) = self.pending_addrs.pop_front() {
            let ev = match update {
                AddrUpdate::Confirmed(addr) => ToSwarm::ExternalAddrConfirmed(addr),
                AddrUpdate::Expired(addr) => ToSwarm::ExternalAddrExpired(addr),
            };
            return Poll::Ready(ev);
        }
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        loop {
            match &mut self.state {
                State::Idle => {
                    if self.can_issue() {
                        self.start_issuance();
                    } else {
                        return Poll::Pending;
                    }
                }
                State::Issuing(receiver) => match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(Ok(certificate))) => {
                        if let Err(error) = self
                            .resolver
                            .set_pem(&certificate.chain_pem, &certificate.key_pem)
                        {
                            tracing::warn!("failed to install obtained certificate: {error}");
                            self.state = State::Backoff(Delay::new(FAILURE_BACKOFF));
                        } else {
                            tracing::info!(
                                not_after = certificate.not_after_unix,
                                "obtained AutoTLS certificate"
                            );
                            self.state = State::Active(renewal_delay(certificate.not_after_unix));
                            self.pending_events.push_back(Event::CertificateObtained {
                                not_after_unix: certificate.not_after_unix,
                            });
                            self.reconcile_advertised();
                        }
                    }
                    Poll::Ready(Ok(Err(error))) => {
                        tracing::warn!("AutoTLS certificate issuance failed: {error}");
                        self.state = State::Backoff(Delay::new(FAILURE_BACKOFF));
                        self.pending_events.push_back(Event::IssuanceFailed(error));
                    }
                    Poll::Ready(Err(_canceled)) => {
                        self.state = State::Backoff(Delay::new(FAILURE_BACKOFF));
                    }
                    Poll::Pending => return Poll::Pending,
                },
                State::Active(delay) | State::Backoff(delay) => {
                    if delay.poll_unpin(cx).is_ready() {
                        self.state = State::Idle;
                    } else {
                        return Poll::Pending;
                    }
                }
            }

            if let Some(update) = self.pending_addrs.pop_front() {
                return Poll::Ready(match update {
                    AddrUpdate::Confirmed(addr) => ToSwarm::ExternalAddrConfirmed(addr),
                    AddrUpdate::Expired(addr) => ToSwarm::ExternalAddrExpired(addr),
                });
            }
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(ToSwarm::GenerateEvent(event));
            }
        }
    }
}

fn renewal_delay(not_after_unix: i64) -> Delay {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let until_expiry = (not_after_unix - now).max(0) as u64;
    Delay::new(Duration::from_secs(
        until_expiry.saturating_sub(RENEWAL_LEAD.as_secs()),
    ))
}

fn extract_ip(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|protocol| match protocol {
        Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
        Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
        _ => None,
    })
}

fn wss_port(addr: &Multiaddr) -> Option<u16> {
    let mut port = None;
    let mut tls = false;
    for protocol in addr.iter() {
        match protocol {
            Protocol::Tcp(p) => port = Some(p),
            Protocol::Tls => tls = true,
            Protocol::Ws(_) if tls => return port,
            Protocol::Wss(_) => return port,
            _ => {}
        }
    }
    None
}

fn forge_wss_addr(ip: IpAddr, port: u16, peer_id_label: &str, forge_domain: &str) -> Multiaddr {
    let host = format!("{}.{peer_id_label}.{forge_domain}", encoding::ip_label(ip));
    let mut addr = Multiaddr::empty();
    addr.push(match ip {
        IpAddr::V4(_) => Protocol::Dns4(host.into()),
        IpAddr::V6(_) => Protocol::Dns6(host.into()),
    });
    addr.push(Protocol::Tcp(port));
    addr.push(Protocol::Tls);
    addr.push(Protocol::Ws("/".into()));
    addr
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn extract_ip_finds_first_ip() {
        let addr: Multiaddr = "/ip4/1.2.3.4/tcp/4001".parse().unwrap();
        assert_eq!(
            extract_ip(&addr),
            Some(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)))
        );
        let addr: Multiaddr = "/dns4/example.com/tcp/443".parse().unwrap();
        assert_eq!(extract_ip(&addr), None);
    }

    #[test]
    fn wss_port_matches_tls_ws_and_wss() {
        assert_eq!(
            wss_port(&"/ip4/1.2.3.4/tcp/443/tls/ws".parse().unwrap()),
            Some(443)
        );
        assert_eq!(
            wss_port(&"/ip4/1.2.3.4/tcp/443/wss".parse().unwrap()),
            Some(443)
        );
        assert_eq!(wss_port(&"/ip4/1.2.3.4/tcp/4001".parse().unwrap()), None);
        assert_eq!(wss_port(&"/ip4/1.2.3.4/tcp/8080/ws".parse().unwrap()), None);
    }

    #[test]
    fn forge_wss_addr_uses_dns4_and_dns6() {
        let v4 = forge_wss_addr(
            IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)),
            443,
            "k51peer",
            "libp2p.direct",
        );
        assert_eq!(
            v4.to_string(),
            "/dns4/1-2-3-4.k51peer.libp2p.direct/tcp/443/tls/ws"
        );
        let v6 = forge_wss_addr(
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            443,
            "k51peer",
            "libp2p.direct",
        );
        assert_eq!(
            v6.to_string(),
            "/dns6/0--1.k51peer.libp2p.direct/tcp/443/tls/ws"
        );
    }
}
