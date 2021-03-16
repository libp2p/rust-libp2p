// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! # libp2p-dns
//!
//! This crate provides the type [`GenDnsConfig`] with its instantiations
//! [`DnsConfig`] and `TokioDnsConfig` for use with `async-std` and `tokio`,
//! respectively.
//!
//! A [`GenDnsConfig`] is a [`Transport`] wrapper that is created around
//! an inner `Transport`. The composed transport behaves like the inner
//! transport, except that [`Transport::dial`] resolves `/dns`, `/dns4/` and
//! `/dns6/` components of a given `Multiaddr` through a DNS.
//!
//! The `async-std` feature and hence the `DnsConfig` are
//! enabled by default. Tokio users can furthermore opt-in
//! to the `tokio-dns-over-rustls` and `tokio-dns-over-https-rustls`
//! features. For more information about these features, please
//! refer to the documentation of [trust-dns-resolver].
//!
//![trust-dns-resolver]: https://docs.rs/trust-dns-resolver/latest/trust_dns_resolver/#dns-over-tls-and-dns-over-https

use futures::{prelude::*, future::BoxFuture, stream::FuturesOrdered};
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{TransportError, ListenerEvent}
};
use log::{debug, trace};
use std::{error, fmt, net::IpAddr};
#[cfg(any(feature = "async-std", feature = "tokio"))]
use std::io;
#[cfg(any(feature = "async-std", feature = "tokio"))]
use trust_dns_resolver::system_conf;
use trust_dns_resolver::{
    AsyncResolver,
    ConnectionProvider,
    proto::xfer::dns_handle::DnsHandle,
};
#[cfg(feature = "tokio")]
use trust_dns_resolver::{TokioAsyncResolver, TokioConnection, TokioConnectionProvider};
#[cfg(feature = "async-std")]
use async_std_resolver::{AsyncStdConnection, AsyncStdConnectionProvider};

pub use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
pub use trust_dns_resolver::error::{ResolveError, ResolveErrorKind};

/// A `Transport` wrapper for performing DNS lookups when dialing `Multiaddr`esses
/// using `async-std` for all async I/O.
#[cfg(feature = "async-std")]
pub type DnsConfig<T> = GenDnsConfig<T, AsyncStdConnection, AsyncStdConnectionProvider>;

/// A `Transport` wrapper for performing DNS lookups when dialing `Multiaddr`esses
/// using `tokio` for all async I/O.
#[cfg(feature = "tokio")]
pub type TokioDnsConfig<T> = GenDnsConfig<T, TokioConnection, TokioConnectionProvider>;

/// A `Transport` wrapper for performing DNS lookups when dialing `Multiaddr`esses.
#[derive(Clone)]
pub struct GenDnsConfig<T, C, P>
where
    C: DnsHandle<Error = ResolveError>,
    P: ConnectionProvider<Conn = C>
{
    /// The underlying transport.
    inner: T,
    /// The DNS resolver used when dialing addresses with DNS components.
    resolver: AsyncResolver<C, P>,
}

#[cfg(feature = "async-std")]
impl<T> DnsConfig<T> {
    /// Creates a new [`DnsConfig`] from the OS's DNS configuration and defaults.
    pub async fn system(inner: T) -> Result<DnsConfig<T>, io::Error> {
        let (cfg, opts) = system_conf::read_system_conf()?;
        Self::custom(inner, cfg, opts).await
    }

    /// Creates a [`DnsConfig`] with a custom resolver configuration and options.
    pub async fn custom(inner: T, cfg: ResolverConfig, opts: ResolverOpts)
        -> Result<DnsConfig<T>, io::Error>
    {
        Ok(DnsConfig {
            inner,
            resolver: async_std_resolver::resolver(cfg, opts).await?
        })
    }
}

#[cfg(feature = "tokio")]
impl<T> TokioDnsConfig<T> {
    /// Creates a new [`TokioDnsConfig`] from the OS's DNS configuration and defaults.
    pub fn system(inner: T) -> Result<TokioDnsConfig<T>, io::Error> {
        let (cfg, opts) = system_conf::read_system_conf()?;
        Self::custom(inner, cfg, opts)
    }

    /// Creates a [`TokioDnsConfig`] with a custom resolver configuration
    /// and options.
    pub fn custom(inner: T, cfg: ResolverConfig, opts: ResolverOpts)
        -> Result<TokioDnsConfig<T>, io::Error>
    {
        Ok(TokioDnsConfig {
            inner,
            resolver: TokioAsyncResolver::tokio(cfg, opts)?
        })
    }
}

impl<T, C, P> fmt::Debug for GenDnsConfig<T, C, P>
where
    C: DnsHandle<Error = ResolveError>,
    P: ConnectionProvider<Conn = C>,
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("GenDnsConfig").field(&self.inner).finish()
    }
}

impl<T, C, P> Transport for GenDnsConfig<T, C, P>
where
    T: Transport + Send + 'static,
    T::Error: Send,
    T::Dial: Send,
    C: DnsHandle<Error = ResolveError>,
    P: ConnectionProvider<Conn = C>,
{
    type Output = T::Output;
    type Error = DnsErr<T::Error>;
    type Listener = stream::MapErr<
        stream::MapOk<T::Listener,
            fn(ListenerEvent<T::ListenerUpgrade, T::Error>)
                -> ListenerEvent<Self::ListenerUpgrade, Self::Error>>,
        fn(T::Error) -> Self::Error>;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, fn(T::Error) -> Self::Error>;
    type Dial = future::Either<
        future::MapErr<T::Dial, fn(T::Error) -> Self::Error>,
        BoxFuture<'static, Result<Self::Output, Self::Error>>
    >;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let listener = self.inner.listen_on(addr).map_err(|err| err.map(DnsErr::Transport))?;
        let listener = listener
            .map_ok::<_, fn(_) -> _>(|event| {
                event
                    .map(|upgr| {
                        upgr.map_err::<_, fn(_) -> _>(DnsErr::Transport)
                    })
                    .map_err(DnsErr::Transport)
            })
            .map_err::<_, fn(_) -> _>(DnsErr::Transport);
        Ok(listener)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // Check if there are any domain names in the address. If not, proceed
        // straight away with dialing on the underlying transport.
        if !addr.iter().any(|p| match p {
            Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) => true,
            _ => false
        }) {
            trace!("Pass-through address without DNS: {}", addr);
            let inner_dial = self.inner.dial(addr)
                .map_err(|err| err.map(DnsErr::Transport))?;
            return Ok(inner_dial.map_err::<_, fn(_) -> _>(DnsErr::Transport).left_future());
        }

        // Asynchronlously resolve all DNS names in the address before proceeding
        // with dialing on the underlying transport.
        Ok(async move {
            let resolver = self.resolver;
            let inner = self.inner;

            trace!("Resolving DNS: {}", addr);

            let resolved = addr.into_iter()
                .map(|proto| resolve(proto, &resolver))
                .collect::<FuturesOrdered<_>>()
                .collect::<Vec<Result<Protocol<'_>, Self::Error>>>()
                .await
                .into_iter()
                .collect::<Result<Vec<Protocol<'_>>, Self::Error>>()?
                .into_iter()
                .collect::<Multiaddr>();

            debug!("DNS resolved: {} => {}", addr, resolved);

            match inner.dial(resolved) {
                Ok(out) => out.await.map_err(DnsErr::Transport),
                Err(TransportError::MultiaddrNotSupported(a)) =>
                    Err(DnsErr::MultiaddrNotSupported(a)),
                Err(TransportError::Other(err)) => Err(DnsErr::Transport(err))
            }
        }.boxed().right_future())
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(server, observed)
    }
}

/// The possible errors of a [`GenDnsConfig`] wrapped transport.
#[derive(Debug)]
pub enum DnsErr<TErr> {
    /// The underlying transport encountered an error.
    Transport(TErr),
    /// DNS resolution failed.
    ResolveError(ResolveError),
    /// DNS resolution was successful, but the underlying transport refused the resolved address.
    MultiaddrNotSupported(Multiaddr),
}

impl<TErr> fmt::Display for DnsErr<TErr>
where TErr: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsErr::Transport(err) => write!(f, "{}", err),
            DnsErr::ResolveError(err) => write!(f, "{}", err),
            DnsErr::MultiaddrNotSupported(a) => write!(f, "Unsupported resolved address: {}", a),
        }
    }
}

impl<TErr> error::Error for DnsErr<TErr>
where TErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DnsErr::Transport(err) => Some(err),
            DnsErr::ResolveError(err) => Some(err),
            DnsErr::MultiaddrNotSupported(_) => None,
        }
    }
}

/// Asynchronously resolves the domain name of a `Dns`, `Dns4` or `Dns6` protocol
/// component. If the given protocol is not a DNS component, it is returned unchanged.
fn resolve<'a, E: 'a, C, P>(proto: Protocol<'a>, resolver: &'a AsyncResolver<C,P>)
    -> impl Future<Output = Result<Protocol<'a>, DnsErr<E>>> + 'a
where
    C: DnsHandle<Error = ResolveError>,
    P: ConnectionProvider<Conn = C>,
{
    match proto {
        Protocol::Dns(ref name) => {
            resolver.lookup_ip(fqdn(name)).map(move |res| match res {
                Ok(ips) => Ok(ips.into_iter()
                    .next()
                    .map(Protocol::from)
                    .expect("If there are no results, `Err(NoRecordsFound)` is expected.")),
                Err(e) => return Err(DnsErr::ResolveError(e))
            }).left_future()
        }
        Protocol::Dns4(ref name) => {
            resolver.ipv4_lookup(fqdn(name)).map(move |res| match res {
                Ok(ips) => Ok(ips.into_iter()
                    .map(IpAddr::from)
                    .next()
                    .map(Protocol::from)
                    .expect("If there are no results, `Err(NoRecordsFound)` is expected.")),
                Err(e) => return Err(DnsErr::ResolveError(e))
            }).left_future().left_future().right_future()
        }
        Protocol::Dns6(ref name) => {
            resolver.ipv6_lookup(fqdn(name)).map(move |res| match res {
                Ok(ips) => Ok(ips.into_iter()
                    .map(IpAddr::from)
                    .next()
                    .map(Protocol::from)
                    .expect("If there are no results, `Err(NoRecordsFound)` is expected.")),
                Err(e) => return Err(DnsErr::ResolveError(e))
            }).right_future().left_future().right_future()
        },
        proto => future::ready(Ok(proto)).right_future().right_future()
    }
}

fn fqdn(name: &std::borrow::Cow<'_, str>) -> String {
    if name.ends_with(".") {
        name.to_string()
    } else {
        format!("{}.", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{future::BoxFuture, stream::BoxStream};
    use libp2p_core::{
        Transport,
        multiaddr::{Protocol, Multiaddr},
        transport::ListenerEvent,
        transport::TransportError,
    };

    #[test]
    fn basic_resolve() {
        let _ = env_logger::try_init();

        #[derive(Clone)]
        struct CustomTransport;

        impl Transport for CustomTransport {
            type Output = ();
            type Error = std::io::Error;
            type Listener = BoxStream<'static, Result<ListenerEvent<Self::ListenerUpgrade, Self::Error>, Self::Error>>;
            type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

            fn listen_on(self, _: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
                unreachable!()
            }

            fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
                let addr = addr.iter().collect::<Vec<_>>();
                assert_eq!(addr.len(), 2);
                match addr[1] {
                    Protocol::Tcp(_) => (),
                    _ => panic!(),
                };
                match addr[0] {
                    Protocol::Ip4(_) => (),
                    Protocol::Ip6(_) => (),
                    _ => panic!(),
                };
                Ok(Box::pin(future::ready(Ok(()))))
            }

            fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
                None
            }
        }

        async fn run<T, C, P>(transport: GenDnsConfig<T, C, P>)
        where
            C: DnsHandle<Error = ResolveError>,
            P: ConnectionProvider<Conn = C>,
            T: Transport + Clone + Send + 'static,
            T::Error: Send,
            T::Dial: Send,
        {
            // Success due to existing A record for example.com.
            let _ = transport
                .clone()
                .dial("/dns4/example.com/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            // Success due to existing AAAA record for example.com.
            let _ = transport
                .clone()
                .dial("/dns6/example.com/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            // Success due to pass-through, i.e. nothing to resolve.
            let _ = transport
                .clone()
                .dial("/ip4/1.2.3.4/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            // Failure due to no records.
            match transport
                .clone()
                .dial("/dns4/example.invalid/tcp/20000".parse().unwrap())
                .unwrap()
                .await
            {
                Err(DnsErr::ResolveError(e)) => match e.kind() {
                    ResolveErrorKind::NoRecordsFound { .. } => {},
                    _ => panic!("Unexpected DNS error: {:?}", e),
                },
                Err(e) => panic!("Unexpected error: {:?}", e),
                Ok(_) => panic!("Unexpected success."),
            }
        }

        #[cfg(feature = "async-std")]
        {
            async_std_crate::task::block_on(
                DnsConfig::system(CustomTransport).then(|dns| run(dns.unwrap()))
            );
        }

        #[cfg(feature = "tokio")]
        {
            let rt = tokio_crate::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .unwrap();
            rt.block_on(run(TokioDnsConfig::system(CustomTransport).unwrap()));
        }
    }
}
