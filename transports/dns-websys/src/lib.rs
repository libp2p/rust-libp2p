//! # DNS name resolution for libp2p under WASM, via DNS-over-HTTPS.
//!
//! This crate provides a [`Transport`] for `wasm32` (browser) targets. Much llike
//! [`libp2p-dns`](https://docs.rs/libp2p-dns), it is an address-rewriting
//! wrapper around an inner [`libp2p_core::Transport`]: on
//! [`libp2p_core::Transport::dial`] it resolves the DNS components of a
//! [`Multiaddr`], replacing them with the resolved protocols before handing the
//! address to the inner transport.
//!
//! Browsers do not expose raw UDP/TCP sockets, so traditional DNS resolution
//! (as used by `libp2p-dns`) is impossible. Instead, this crate resolves names
//! over [DNS-over-HTTPS](https://datatracker.ietf.org/doc/html/rfc8484) using
//! the browser's `fetch` API and a JSON (`application/dns-json`) endpoint. The
//! endpoint is configurable via [`Config`] and defaults to Cloudflare.
//!
//! `/dnsaddr` is always resolved (browsers cannot look up TXT records, so this
//! is the gap worth filling such as dialing `/dnsaddr/bootstrap.libp2p.io`).
//! `/dns`, `/dns4` and `/dns6` are governed by [`DnsResolution`], which defaults
//! to [`DnsResolution::Auto`]: addresses containing a `/webrtc-direct` (or any future specific
//! protocols) are resolved to `/ip4`/`/ip6` (that transport needs a numeric IP), while
//! everything else is passed through unchanged, because the name-bound TLS
//! transports (WebSocket, WebTransport) resolve hostnames natively and need the
//! hostname preserved for SNI and certificate validation. Override via
//! [`Config::dns_resolution`].

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod resolver;
mod web_context;

use std::{
    error, fmt, io,
    ops::DerefMut,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::{future, prelude::*};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{DialOpts, ListenerId, TransportError, TransportEvent},
};
use parking_lot::Mutex;
use send_wrapper::SendWrapper;
use smallvec::SmallVec;

use crate::resolver::Resolver;
pub use crate::resolver::{CLOUDFLARE, Config, DnsResolution, GOOGLE, ResolveError};

/// The prefix for `dnsaddr` protocol TXT record lookups.
const DNSADDR_PREFIX: &str = "_dnsaddr.";

/// The maximum number of dialing attempts to resolved addresses.
const MAX_DIAL_ATTEMPTS: usize = 16;

/// The maximum number of DNS lookups when dialing.
///
/// This limit is primarily a safeguard against too many, possibly even cyclic,
/// indirections in the addresses obtained from the TXT records of a `/dnsaddr`.
const MAX_DNS_LOOKUPS: usize = 32;

/// The maximum number of TXT records applicable for the address being dialed
/// that are considered for further lookups as a result of a single `/dnsaddr`
/// lookup.
const MAX_TXT_RECORDS: usize = 16;

/// A [`libp2p_core::Transport`] that resolves DNS names over HTTPS before
/// dialing the inner transport. Intended for `wasm32` (browser) targets.
#[derive(Debug)]
pub struct Transport<T> {
    /// The underlying transport.
    inner: Arc<Mutex<T>>,
    /// The DoH resolver used when dialing addresses with DNS
    /// components.
    resolver: Resolver,
}

impl<T> Transport<T> {
    /// Creates a new [`Transport`] using the default ([`Config::cloudflare`])
    /// DoH endpoint.
    pub fn new(inner: T) -> Self {
        Self::with_config(inner, Config::default())
    }

    /// Creates a new [`Transport`] using the given DoH [`Config`].
    pub fn with_config(inner: T, config: Config) -> Self {
        Transport {
            inner: Arc::new(Mutex::new(inner)),
            resolver: Resolver::new(config),
        }
    }
}

impl<T> libp2p_core::Transport for Transport<T>
where
    T: libp2p_core::Transport + Send + Unpin + 'static,
    T::Error: Send,
    T::Dial: Send,
{
    type Output = T::Output;
    type Error = Error<T::Error>;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, fn(T::Error) -> Self::Error>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.inner
            .lock()
            .listen_on(id, addr)
            .map_err(|e| e.map(Error::Transport))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.lock().remove_listener(id)
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Ok(self.do_dial(addr, dial_opts))
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let mut inner = self.inner.lock();
        libp2p_core::Transport::poll(Pin::new(inner.deref_mut()), cx).map(|event| {
            event
                .map_upgrade(|upgr| upgr.map_err::<_, fn(_) -> _>(Error::Transport))
                .map_err(Error::Transport)
        })
    }
}

impl<T> Transport<T>
where
    T: libp2p_core::Transport + Send + Unpin + 'static,
    T::Error: Send,
    T::Dial: Send,
{
    fn do_dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> <Self as libp2p_core::Transport>::Dial {
        let resolver = self.resolver.clone();
        let inner = self.inner.clone();
        let dns_resolution = self.resolver.dns_resolution();

        // The lookups are driven by the browser's `fetch` API, whose futures are
        // `!Send`. `SendWrapper` makes the resulting future `Send` (sound on the
        // single-threaded wasm runtime), so it satisfies the bound on `Dial`.
        SendWrapper::new(async move {
            let mut dial_errors: Vec<Error<T::Error>> = Vec::new();
            let mut dns_lookups = 0;
            let mut dial_attempts = 0;
            // We optimise for the common case of a single DNS component in the
            // address that is resolved with a single lookup.
            let mut unresolved = SmallVec::<[Multiaddr; 1]>::new();
            unresolved.push(addr.clone());

            // Resolve (i.e. replace) all DNS protocol components, initiating
            // dialing attempts as soon as there is another fully resolved
            // address.
            while let Some(addr) = unresolved.pop() {
                let resolve_dns = should_resolve_dns(&addr, dns_resolution);
                if let Some((i, name)) = addr
                    .iter()
                    .enumerate()
                    .find(|(_, p)| is_resolvable(p, resolve_dns))
                {
                    if dns_lookups == MAX_DNS_LOOKUPS {
                        tracing::debug!(address=%addr, "Too many DNS lookups, dropping unresolved address");
                        dial_errors.push(Error::TooManyLookups);
                        // There may still be fully resolved addresses in
                        // `unresolved`, so keep going until it is empty.
                        continue;
                    }
                    dns_lookups += 1;
                    match resolve(&name, &resolver).await {
                        Err(e) => {
                            // Record the resolution error.
                            dial_errors.push(e);
                        }
                        Ok(Resolved::One(ip)) => {
                            tracing::trace!(protocol=%name, resolved=%ip);
                            let addr = addr.replace(i, |_| Some(ip)).expect("`i` is a valid index");
                            unresolved.push(addr);
                        }
                        Ok(Resolved::Many(ips)) => {
                            for ip in ips {
                                tracing::trace!(protocol=%name, resolved=%ip);
                                let addr =
                                    addr.replace(i, |_| Some(ip)).expect("`i` is a valid index");
                                unresolved.push(addr);
                            }
                        }
                        Ok(Resolved::Addrs(addrs)) => {
                            let suffix = addr.iter().skip(i + 1).collect::<Multiaddr>();
                            let prefix = addr.iter().take(i).collect::<Multiaddr>();
                            let mut n = 0;
                            for a in addrs {
                                if a.ends_with(&suffix) {
                                    if n < MAX_TXT_RECORDS {
                                        n += 1;
                                        tracing::trace!(protocol=%name, resolved=%a);
                                        let addr =
                                            prefix.iter().chain(a.iter()).collect::<Multiaddr>();
                                        unresolved.push(addr);
                                    } else {
                                        tracing::debug!(
                                            resolved=%a,
                                            "Too many TXT records, dropping resolved"
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // We have a fully resolved address, so try to dial it.
                    tracing::debug!(address=%addr, "Dialing address");

                    let transport = inner.clone();
                    let dial = transport.lock().dial(addr, dial_opts);
                    let result = match dial {
                        Ok(out) => {
                            // We only count attempts that the inner transport
                            // actually accepted, i.e. for which it produced a
                            // dialing future.
                            dial_attempts += 1;
                            out.await.map_err(Error::Transport)
                        }
                        Err(TransportError::MultiaddrNotSupported(a)) => {
                            Err(Error::MultiaddrNotSupported(a))
                        }
                        Err(TransportError::Other(err)) => Err(Error::Transport(err)),
                    };

                    match result {
                        Ok(out) => return Ok(out),
                        Err(err) => {
                            tracing::debug!("Dial error: {:?}.", err);
                            dial_errors.push(err);

                            if unresolved.is_empty() {
                                break;
                            }

                            if dial_attempts == MAX_DIAL_ATTEMPTS {
                                tracing::debug!(
                                    "Aborting dialing after {} attempts.",
                                    MAX_DIAL_ATTEMPTS
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // If we have any dial errors, aggregate them. Otherwise there were
            // no valid DNS records for the given address to begin with.
            if !dial_errors.is_empty() {
                Err(Error::Dial(dial_errors))
            } else {
                Err(Error::ResolveError(ResolveError::Fetch(
                    "no matching records found".to_owned(),
                )))
            }
        })
        .boxed()
    }
}

/// The possible errors of a [`Transport`]-wrapped transport.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Error<TErr> {
    /// The underlying transport encountered an error.
    Transport(TErr),
    /// DNS resolution failed.
    #[allow(clippy::enum_variant_names)]
    ResolveError(ResolveError),
    /// DNS resolution was successful, but the underlying transport refused the
    /// resolved address.
    MultiaddrNotSupported(Multiaddr),
    /// DNS resolution involved too many lookups.
    TooManyLookups,
    /// Multiple dial errors were encountered.
    Dial(Vec<Error<TErr>>),
}

impl<TErr> fmt::Display for Error<TErr>
where
    TErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(err) => write!(f, "{err}"),
            Error::ResolveError(err) => write!(f, "{err}"),
            Error::MultiaddrNotSupported(a) => write!(f, "Unsupported resolved address: {a}"),
            Error::TooManyLookups => write!(f, "Too many DNS lookups"),
            Error::Dial(errs) => {
                write!(f, "Multiple dial errors occurred:")?;
                for err in errs {
                    write!(f, "\n - {err}")?;
                }
                Ok(())
            }
        }
    }
}

impl<TErr> error::Error for Error<TErr>
where
    TErr: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Transport(err) => Some(err),
            Error::ResolveError(err) => Some(err),
            Error::MultiaddrNotSupported(_) => None,
            Error::TooManyLookups => None,
            Error::Dial(errs) => errs.last().and_then(|e| e.source()),
        }
    }
}

/// The successful outcome of [`resolve`] for a given [`Protocol`].
enum Resolved<'a> {
    /// The given `Protocol` has been resolved to a single `Protocol`, which may
    /// be identical to the one given, in case it is not a DNS protocol
    /// component.
    One(Protocol<'a>),
    /// The given `Protocol` has been resolved to multiple alternative
    /// `Protocol`s as a result of a DNS lookup.
    Many(Vec<Protocol<'a>>),
    /// The given `Protocol` has been resolved to a new list of `Multiaddr`s
    /// obtained from DNS TXT records representing possible alternatives. These
    /// addresses may contain further DNS names that need resolving.
    Addrs(Vec<Multiaddr>),
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
async fn resolve<'a, E>(
    proto: &Protocol<'a>,
    resolver: &Resolver,
) -> Result<Resolved<'a>, Error<E>> {
    match proto {
        Protocol::Dns(name) => {
            // `/dns` resolves to both A and AAAA records; tolerate one family
            // failing as long as the other yields a result.
            let v4 = resolver.ipv4_lookup(name.as_ref()).await;
            let v6 = resolver.ipv6_lookup(name.as_ref()).await;
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
                .ipv4_lookup(name.as_ref())
                .await
                .map_err(Error::ResolveError)?;
            collect(ips.into_iter().map(Protocol::from).collect())
        }
        Protocol::Dns6(name) => {
            let ips = resolver
                .ipv6_lookup(name.as_ref())
                .await
                .map_err(Error::ResolveError)?;
            collect(ips.into_iter().map(Protocol::from).collect())
        }
        Protocol::Dnsaddr(name) => {
            let lookup = [DNSADDR_PREFIX, name].concat();
            let txts = resolver
                .txt_lookup(&lookup)
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
        0 => Err(Error::ResolveError(ResolveError::Fetch(
            "no matching records found".to_owned(),
        ))),
        1 => Ok(Resolved::One(protocols.remove(0))),
        _ => Ok(Resolved::Many(protocols)),
    }
}

/// Parses a `<character-string>` of a `dnsaddr` TXT record.
fn parse_dnsaddr_txt(txt: &str) -> io::Result<Multiaddr> {
    match txt.strip_prefix("dnsaddr=") {
        None => Err(invalid_data("Missing `dnsaddr=` prefix.")),
        Some(a) => Ok(Multiaddr::try_from(a).map_err(invalid_data)?),
    }
}

fn invalid_data(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnsaddr_is_always_resolvable() {
        let dnsaddr = Protocol::Dnsaddr("bootstrap.libp2p.io".into());
        assert!(is_resolvable(&dnsaddr, false));
        assert!(is_resolvable(&dnsaddr, true));

        let dns4 = Protocol::Dns4("example.com".into());
        assert!(!is_resolvable(&dns4, false));
        assert!(is_resolvable(&dns4, true));
    }

    #[test]
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

    #[test]
    fn parse_dnsaddr_txt_requires_prefix() {
        let addr = parse_dnsaddr_txt("dnsaddr=/dns4/example.com/tcp/443/wss").unwrap();
        assert_eq!(
            addr,
            "/dns4/example.com/tcp/443/wss"
                .parse::<Multiaddr>()
                .unwrap()
        );
        assert!(parse_dnsaddr_txt("/dns4/example.com").is_err());
    }
}
