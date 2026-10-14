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

//! DNS name resolution through [hickory-resolver](https://docs.rs/hickory-resolver).

#[cfg(feature = "tokio")]
pub mod tokio {
    use std::sync::Arc;

    use hickory_resolver::{TokioResolver, net::runtime::TokioRuntimeProvider, system_conf};
    use parking_lot::Mutex;

    /// A `Transport` wrapper for performing DNS lookups when dialing `Multiaddr`esses
    /// using `tokio` for all async I/O.
    pub type Transport<T> = crate::Transport<T, TokioResolver>;

    impl<T> Transport<T> {
        /// Creates a new [`Transport`] from the OS's DNS configuration and defaults.
        pub fn system(inner: T) -> Result<Transport<T>, std::io::Error> {
            let (cfg, opts) = system_conf::read_system_conf()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(Self::custom(inner, cfg, opts))
        }

        /// Creates a [`Transport`] with a custom resolver configuration
        /// and options.
        pub fn custom(
            inner: T,
            cfg: hickory_resolver::config::ResolverConfig,
            opts: hickory_resolver::config::ResolverOpts,
        ) -> Transport<T> {
            Transport {
                inner: Arc::new(Mutex::new(inner)),
                resolver: TokioResolver::builder_with_config(cfg, TokioRuntimeProvider::default())
                    .with_options(opts)
                    .build()
                    .expect("valid resolver config should build"),
            }
        }
    }
}

use std::{
    iter,
    net::{Ipv4Addr, Ipv6Addr},
    str,
};

use hickory_resolver::{ConnectionProvider, lookup::Lookup, lookup_ip::LookupIp, proto::rr::RData};
pub use hickory_resolver::{
    config::{ResolverConfig, ResolverOpts},
    net::NetError as ResolveError,
};
use libp2p_core::multiaddr::{Multiaddr, Protocol};

use crate::{DNSADDR_PREFIX, Error, Resolved, invalid_data, parse_dnsaddr_txt};

#[doc(hidden)]
pub trait Resolver {
    fn lookup_ip(
        &self,
        name: String,
    ) -> impl Future<Output = Result<LookupIp, ResolveError>> + Send;
    fn ipv4_lookup(
        &self,
        name: String,
    ) -> impl Future<Output = Result<Lookup, ResolveError>> + Send;
    fn ipv6_lookup(
        &self,
        name: String,
    ) -> impl Future<Output = Result<Lookup, ResolveError>> + Send;
    fn txt_lookup(&self, name: String)
    -> impl Future<Output = Result<Lookup, ResolveError>> + Send;
}

impl<C> Resolver for hickory_resolver::Resolver<C>
where
    C: ConnectionProvider,
{
    async fn lookup_ip(&self, name: String) -> Result<LookupIp, ResolveError> {
        self.lookup_ip(name).await
    }

    async fn ipv4_lookup(&self, name: String) -> Result<Lookup, ResolveError> {
        self.ipv4_lookup(name).await
    }

    async fn ipv6_lookup(&self, name: String) -> Result<Lookup, ResolveError> {
        self.ipv6_lookup(name).await
    }

    async fn txt_lookup(&self, name: String) -> Result<Lookup, ResolveError> {
        self.txt_lookup(name).await
    }
}

/// The error reported when a lookup succeeded but yielded no record applicable
/// to the address being dialed.
pub(crate) fn no_records_found() -> ResolveError {
    ResolveError::from("No Matching Records Found")
}

/// Returns the next DNS protocol component of `addr` that needs resolving.
pub(crate) fn next_unresolved<'a, R>(
    addr: &'a Multiaddr,
    _resolver: &R,
) -> Option<(usize, Protocol<'a>)>
where
    R: Resolver,
{
    addr.iter().enumerate().find(|(_, p)| {
        matches!(
            p,
            Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) | Protocol::Dnsaddr(_)
        )
    })
}

/// Asynchronously resolves the domain name of a `Dns`, `Dns4`, `Dns6` or `Dnsaddr` protocol
/// component. If the given protocol is of a different type, it is returned unchanged as a
/// [`Resolved::One`].
pub(crate) async fn resolve<'a, E, R>(
    proto: &Protocol<'a>,
    resolver: &R,
) -> Result<Resolved<'a>, Error<E>>
where
    R: Resolver,
{
    match proto {
        Protocol::Dns(name) => {
            let lookup = resolver
                .lookup_ip(name.clone().into_owned())
                .await
                .map_err(Error::ResolveError)?;
            let mut ips = lookup.iter();
            let one = ips
                .next()
                .expect("If there are no results, `Err(NoRecordsFound)` is expected.");
            if let Some(two) = ips.next() {
                Ok(Resolved::Many(
                    iter::once(one)
                        .chain(iter::once(two))
                        .chain(ips)
                        .map(Protocol::from)
                        .collect(),
                ))
            } else {
                Ok(Resolved::One(Protocol::from(one)))
            }
        }
        Protocol::Dns4(name) => {
            let lookup = resolver
                .ipv4_lookup(name.clone().into_owned())
                .await
                .map_err(Error::ResolveError)?;
            let mut ips = lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::A(ip) => Some(Ipv4Addr::from(*ip)),
                    _ => None,
                });
            let one = ips
                .next()
                .expect("If there are no results, `Err(NoRecordsFound)` is expected.");
            if let Some(two) = ips.next() {
                Ok(Resolved::Many(
                    iter::once(one)
                        .chain(iter::once(two))
                        .chain(ips)
                        .map(Protocol::from)
                        .collect(),
                ))
            } else {
                Ok(Resolved::One(Protocol::from(one)))
            }
        }
        Protocol::Dns6(name) => {
            let lookup = resolver
                .ipv6_lookup(name.clone().into_owned())
                .await
                .map_err(Error::ResolveError)?;
            let mut ips = lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::AAAA(ip) => Some(Ipv6Addr::from(*ip)),
                    _ => None,
                });
            let one = ips
                .next()
                .expect("If there are no results, `Err(NoRecordsFound)` is expected.");
            if let Some(two) = ips.next() {
                Ok(Resolved::Many(
                    iter::once(one)
                        .chain(iter::once(two))
                        .chain(ips)
                        .map(Protocol::from)
                        .collect(),
                ))
            } else {
                Ok(Resolved::One(Protocol::from(one)))
            }
        }
        Protocol::Dnsaddr(name) => {
            let name = [DNSADDR_PREFIX, name].concat();
            let txts = resolver
                .txt_lookup(name)
                .await
                .map_err(Error::ResolveError)?;
            let mut addrs = Vec::new();
            for txt in txts
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::TXT(txt) => Some(txt),
                    _ => None,
                })
            {
                if let Some(chars) = txt.txt_data.first() {
                    match str::from_utf8(chars)
                        .map_err(invalid_data)
                        .and_then(parse_dnsaddr_txt)
                    {
                        Err(e) => {
                            // Skip over seemingly invalid entries.
                            tracing::debug!("Invalid TXT record: {:?}", e);
                        }
                        Ok(a) => {
                            addrs.push(a);
                        }
                    }
                }
            }
            Ok(Resolved::Addrs(addrs))
        }
        proto => Ok(Resolved::One(proto.clone())),
    }
}

#[cfg(all(test, feature = "tokio"))]
mod tests {
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };

    use futures::{future, future::BoxFuture, prelude::*};
    use hickory_resolver::config::QUAD9;
    use libp2p_core::{
        Endpoint, Transport,
        multiaddr::{Multiaddr, Protocol},
        transport::{DialOpts, ListenerId, PortUse, TransportError, TransportEvent},
    };
    use libp2p_identity::PeerId;

    use super::*;
    use crate::Error;

    fn test_tokio<T, F: Future<Output = ()>>(
        transport: T,
        test_fn: impl FnOnce(tokio::Transport<T>) -> F,
    ) {
        let config = ResolverConfig::udp_and_tcp(&QUAD9);
        let opts = ResolverOpts::default();
        let transport = tokio::Transport::custom(transport, config, opts);
        let rt = ::tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(test_fn(transport));
    }

    #[test]
    fn basic_resolve() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        #[derive(Clone)]
        struct CustomTransport;

        impl Transport for CustomTransport {
            type Output = ();
            type Error = std::io::Error;
            type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

            fn listen_on(
                &mut self,
                _: ListenerId,
                _: Multiaddr,
            ) -> Result<(), TransportError<Self::Error>> {
                unreachable!()
            }

            fn remove_listener(&mut self, _: ListenerId) -> bool {
                false
            }

            fn dial(
                &mut self,
                addr: Multiaddr,
                _: DialOpts,
            ) -> Result<Self::Dial, TransportError<Self::Error>> {
                // Check that all DNS components have been resolved, i.e. replaced.
                assert!(!addr.iter().any(|p| matches!(
                    p,
                    Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) | Protocol::Dnsaddr(_)
                )));
                Ok(Box::pin(future::ready(Ok(()))))
            }

            fn poll(
                self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
                unreachable!()
            }
        }

        async fn run<T, R>(mut transport: crate::Transport<T, R>)
        where
            T: Transport + Clone + Send + Unpin + 'static,
            T::Error: Send,
            T::Dial: Send,
            R: Clone + Send + Sync + Resolver + 'static,
        {
            let dial_opts = DialOpts {
                role: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            };

            // Success due to existing A record for example.com.
            let _ = transport
                .dial("/dns4/example.com/tcp/20000".parse().unwrap(), dial_opts)
                .unwrap()
                .await
                .unwrap();

            // Success due to existing AAAA record for example.com.
            let _ = transport
                .dial("/dns6/example.com/tcp/20000".parse().unwrap(), dial_opts)
                .unwrap()
                .await
                .unwrap();

            // Success due to pass-through, i.e. nothing to resolve.
            let _ = transport
                .dial("/ip4/1.2.3.4/tcp/20000".parse().unwrap(), dial_opts)
                .unwrap()
                .await
                .unwrap();

            // Success due to the DNS TXT records at _dnsaddr.bootstrap.libp2p.io.
            let _ = transport
                .dial("/dnsaddr/bootstrap.libp2p.io".parse().unwrap(), dial_opts)
                .unwrap()
                .await
                .unwrap();

            // Success due to the DNS TXT records at _dnsaddr.bootstrap.libp2p.io having
            // an entry with suffix `/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN`,
            // i.e. a bootnode with such a peer ID.
            let _ = transport
                .dial("/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN".parse().unwrap(), dial_opts)
                .unwrap()
                .await
                .unwrap();

            // Failure due to the DNS TXT records at _dnsaddr.libp2p.io not having
            // an entry with a random `p2p` suffix.
            match transport
                .dial(
                    format!("/dnsaddr/bootstrap.libp2p.io/p2p/{}", PeerId::random())
                        .parse()
                        .unwrap(),
                    dial_opts,
                )
                .unwrap()
                .await
            {
                Err(Error::ResolveError(_)) => {}
                Err(e) => panic!("Unexpected error: {e:?}"),
                Ok(_) => panic!("Unexpected success."),
            }

            // Failure due to no records.
            match transport
                .dial(
                    "/dns4/example.invalid/tcp/20000".parse().unwrap(),
                    dial_opts,
                )
                .unwrap()
                .await
            {
                Err(Error::Dial(dial_errs)) => {
                    assert_eq!(
                        dial_errs.len(),
                        1,
                        "Expected exactly 1 error for 'no records' scenario, got {dial_errs:?}"
                    );

                    match &dial_errs[0] {
                        Error::ResolveError(e) if e.is_no_records_found() => {}
                        Error::ResolveError(e) => panic!("Unexpected DNS error: {e:?}"),
                        other => {
                            panic!("Expected a single ResolveError(...) sub-error, got {other:?}")
                        }
                    }
                }

                Err(e) => panic!("Unexpected error: {e:?}"),
                Ok(_) => panic!("Unexpected success."),
            }
        }

        test_tokio(CustomTransport, run);
    }

    #[test]
    fn aggregated_dial_errors() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        #[derive(Clone)]
        struct AlwaysFailTransport;

        impl libp2p_core::Transport for AlwaysFailTransport {
            type Output = ();
            type Error = std::io::Error;
            type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

            fn listen_on(
                &mut self,
                _id: ListenerId,
                _addr: Multiaddr,
            ) -> Result<(), TransportError<Self::Error>> {
                unimplemented!()
            }

            fn remove_listener(&mut self, _id: ListenerId) -> bool {
                false
            }

            fn dial(
                &mut self,
                addr: Multiaddr,
                _: DialOpts,
            ) -> Result<Self::Dial, TransportError<Self::Error>> {
                // Every dial attempt fails with an error that includes the address.
                Ok(Box::pin(future::ready(Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("No support for dialing {addr}"),
                )))))
            }

            fn poll(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
                unimplemented!()
            }
        }

        async fn run_test<T, R>(mut transport: crate::Transport<T, R>)
        where
            T: Transport<Error = std::io::Error> + Clone + Send + Unpin + 'static,
            T::Error: Send,
            T::Dial: Send,
            R: Clone + Send + Sync + Resolver + 'static,
        {
            let dial_opts = DialOpts {
                role: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            };

            // This address requires DNS resolution, yielding two IP addresses,
            // forcing two dial attempts. Both fail.
            let addr: Multiaddr = "/dnsaddr/bootstrap.libp2p.io".parse().unwrap();
            let dial_future = transport.dial(addr, dial_opts).unwrap();
            let result = dial_future.await;

            match result {
                Err(Error::Dial(errs)) => {
                    // We expect at least 2 errors, one per resolved IP.
                    assert!(
                        errs.len() >= 2,
                        "Expected multiple dial errors, but got {}",
                        errs.len()
                    );
                    for e in errs {
                        match e {
                            Error::Transport(io_err) => {
                                assert_eq!(
                                    io_err.kind(),
                                    io::ErrorKind::Unsupported,
                                    "Expected Unsupported dial error, got: {io_err:?}"
                                );
                            }
                            _ => panic!("Expected Error::Transport(Unsupported), got: {e:?}"),
                        }
                    }
                }
                Err(e) => panic!("Expected aggregated dial errors, got {e:?}"),
                Ok(_) => panic!("Dial unexpectedly succeeded"),
            }
        }

        test_tokio(AlwaysFailTransport, run_test);
    }
}
