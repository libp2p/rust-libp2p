//! DNS name resolution for `wasm32` targets, via DNS-over-HTTPS.
//!
//! Browsers do not expose raw UDP/TCP sockets, so traditional DNS resolution
//! (as used on non-`wasm32` targets) is impossible. Instead, this module
//! resolves names over
//! [DNS-over-HTTPS](https://datatracker.ietf.org/doc/html/rfc8484) using the
//! browser's `fetch` API and a JSON (`application/dns-json`) endpoint. The
//! endpoint is configurable via [`Config`] and defaults to Cloudflare.
//!
//! `/dnsaddr` is always resolved (browsers cannot look up TXT records, so this
//! is the gap worth filling such as dialing `/dnsaddr/bootstrap.libp2p.io`).
//! `/dns`, `/dns4` and `/dns6` are governed by [`DnsResolution`], which defaults
//! to [`DnsResolution::Auto`]. Addresses containing a `/webrtc-direct` (or any future specific
//! protocols) are resolved to `/ip4`/`/ip6` (that transport needs a numeric IP), while
//! everything else is passed through unchanged, because the name-bound TLS
//! transports (WebSocket, WebTransport) resolve hostnames natively and need the
//! hostname preserved for SNI and certificate validation. Override via
//! [`Config::dns_resolution`].

mod resolver;
mod web_context;

use std::sync::Arc;

use libp2p_core::multiaddr::{Multiaddr, Protocol};
use parking_lot::Mutex;

pub use crate::websys::resolver::{
    CLOUDFLARE, Config, DnsResolution, DohResolver, GOOGLE, ResolveError, Resolver,
};
use crate::{DNSADDR_PREFIX, Error, Resolved, parse_dnsaddr_txt};

/// A `Transport` wrapper for performing DNS lookups over HTTPS when dialing
/// `Multiaddr`esses from within a browser.
pub type Transport<T> = crate::Transport<T, DohResolver>;

impl<T> Transport<T> {
    /// Creates a new [`Transport`] using the default ([`Config::cloudflare`])
    /// DoH endpoint.
    pub fn new(inner: T) -> Self {
        Self::with_config(inner, Config::default())
    }

    /// Creates a new [`Transport`] using the given DoH [`Config`].
    pub fn with_config(inner: T, config: Config) -> Self {
        crate::Transport {
            inner: Arc::new(Mutex::new(inner)),
            resolver: DohResolver::new(config),
        }
    }
}

/// The error reported when a lookup succeeded but yielded no record applicable
/// to the address being dialed.
pub(crate) fn no_records_found() -> ResolveError {
    ResolveError::Fetch("no matching records found".to_owned())
}

/// Returns the next DNS protocol component of `addr` that needs resolving,
/// honouring the resolver's [`DnsResolution`] policy.
pub(crate) fn next_unresolved<'a, R>(
    addr: &'a Multiaddr,
    resolver: &R,
) -> Option<(usize, Protocol<'a>)>
where
    R: Resolver,
{
    let resolve_dns = should_resolve_dns(addr, resolver.dns_resolution());
    addr.iter()
        .enumerate()
        .find(|(_, p)| is_resolvable(p, resolve_dns))
}

fn should_resolve_dns(addr: &Multiaddr, policy: DnsResolution) -> bool {
    match policy {
        DnsResolution::Always => true,
        DnsResolution::Never => false,
        DnsResolution::Auto => addr.iter().any(|p| matches!(p, Protocol::WebRTCDirect)),
    }
}

fn is_resolvable(proto: &Protocol<'_>, resolve_dns: bool) -> bool {
    match proto {
        Protocol::Dnsaddr(_) => true,
        Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) => resolve_dns,
        _ => false,
    }
}

/// Asynchronously resolves the domain name of a `Dns`, `Dns4`, `Dns6` or
/// `Dnsaddr` protocol component. If the given protocol is of a different type,
/// it is returned unchanged as a [`Resolved::One`].
pub(crate) async fn resolve<'a, E, R>(
    proto: &Protocol<'a>,
    resolver: &R,
) -> Result<Resolved<'a>, Error<E>>
where
    R: Resolver,
{
    match proto {
        Protocol::Dns(name) => {
            // `/dns` resolves to both A and AAAA records; tolerate one family
            // failing as long as the other yields a result.
            let v4 = resolver.ipv4_lookup(name.clone().into_owned()).await;
            let v6 = resolver.ipv6_lookup(name.clone().into_owned()).await;
            if let (Err(e), Err(_)) = (&v4, &v6) {
                return Err(Error::ResolveError(e.clone()));
            }
            let mut ips: Vec<Protocol<'a>> = Vec::new();
            ips.extend(v4.into_iter().flatten().map(Protocol::from));
            ips.extend(v6.into_iter().flatten().map(Protocol::from));
            collect(ips)
        }
        Protocol::Dns4(name) => {
            let ips = resolver
                .ipv4_lookup(name.clone().into_owned())
                .await
                .map_err(Error::ResolveError)?;
            collect(ips.into_iter().map(Protocol::from).collect())
        }
        Protocol::Dns6(name) => {
            let ips = resolver
                .ipv6_lookup(name.clone().into_owned())
                .await
                .map_err(Error::ResolveError)?;
            collect(ips.into_iter().map(Protocol::from).collect())
        }
        Protocol::Dnsaddr(name) => {
            let lookup = [DNSADDR_PREFIX, name].concat();
            let txts = resolver
                .txt_lookup(lookup)
                .await
                .map_err(Error::ResolveError)?;
            let mut addrs = Vec::new();
            for txt in txts {
                match parse_dnsaddr_txt(&txt) {
                    Ok(a) => addrs.push(a),
                    // Skip over seemingly invalid entries.
                    Err(e) => tracing::debug!("Invalid TXT record: {:?}", e),
                }
            }
            Ok(Resolved::Addrs(addrs))
        }
        proto => Ok(Resolved::One(proto.clone())),
    }
}

/// Turns the resolved protocols into a [`Resolved`], erroring if empty.
fn collect<'a, E>(mut protocols: Vec<Protocol<'a>>) -> Result<Resolved<'a>, Error<E>> {
    match protocols.len() {
        0 => Err(Error::ResolveError(no_records_found())),
        1 => Ok(Resolved::One(protocols.remove(0))),
        _ => Ok(Resolved::Many(protocols)),
    }
}

#[cfg(test)]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn dnsaddr_is_always_resolvable() {
        let dnsaddr = Protocol::Dnsaddr("bootstrap.libp2p.io".into());
        assert!(is_resolvable(&dnsaddr, false));
        assert!(is_resolvable(&dnsaddr, true));

        let dns4 = Protocol::Dns4("example.com".into());
        assert!(!is_resolvable(&dns4, false));
        assert!(is_resolvable(&dns4, true));
    }

    #[wasm_bindgen_test]
    fn auto_resolves_dns_only_for_webrtc_direct() {
        let wss: Multiaddr = "/dns4/example.com/tcp/443/wss".parse().unwrap();
        let webrtc: Multiaddr =
            "/dns4/example.com/udp/4001/webrtc-direct/certhash/uEiDDq4_xNyDorZBH3TlGazyJdOWSwvo4PUo0dVwsfStPnQ"
                .parse()
                .unwrap();

        assert!(!should_resolve_dns(&wss, DnsResolution::Auto));
        assert!(should_resolve_dns(&webrtc, DnsResolution::Auto));

        assert!(should_resolve_dns(&wss, DnsResolution::Always));
        assert!(should_resolve_dns(&webrtc, DnsResolution::Always));

        assert!(!should_resolve_dns(&wss, DnsResolution::Never));
        assert!(!should_resolve_dns(&webrtc, DnsResolution::Never));
    }
}
