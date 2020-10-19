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
//! This crate provides the type `DnsConfig` that allows one to resolve the `/dns4/` and `/dns6/`
//! components of multiaddresses.
//!
//! ## Usage
//!
//! In order to use this crate, create a `DnsConfig` with one of its constructors and pass it an
//! implementation of the `Transport` trait.
//!
//! Whenever we want to dial an address through the `DnsConfig` and that address contains a
//! `/dns/`, `/dns4/`, or `/dns6/` component, a DNS resolve will be performed and the component
//! will be replaced with `/ip4/` and/or `/ip6/` components.
//!

use futures::{prelude::*, channel::oneshot, future::BoxFuture};
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{TransportError, ListenerEvent}
};
use log::{error, debug, trace};
use std::{error, fmt, io, net::ToSocketAddrs};

/// Represents the configuration for a DNS transport capability of libp2p.
///
/// This struct implements the `Transport` trait and holds an underlying transport. Any call to
/// `dial` with a multiaddr that contains `/dns/`, `/dns4/`, or `/dns6/` will be first be resolved,
/// then passed to the underlying transport.
///
/// Listening is unaffected.
#[derive(Clone)]
pub struct DnsConfig<T> {
    /// Underlying transport to use once the DNS addresses have been resolved.
    inner: T,
    /// Pool of threads to use when resolving DNS addresses.
    thread_pool: futures::executor::ThreadPool,
}

impl<T> DnsConfig<T> {
    /// Creates a new configuration object for DNS.
    pub fn new(inner: T) -> Result<DnsConfig<T>, io::Error> {
        DnsConfig::with_resolve_threads(inner, 1)
    }

    /// Same as `new`, but allows specifying a number of threads for the resolving.
    pub fn with_resolve_threads(inner: T, num_threads: usize) -> Result<DnsConfig<T>, io::Error> {
        let thread_pool = futures::executor::ThreadPool::builder()
            .pool_size(num_threads)
            .name_prefix("libp2p-dns-")
            .create()?;

        trace!("Created a DNS thread pool");

        Ok(DnsConfig {
            inner,
            thread_pool,
        })
    }
}

impl<T> fmt::Debug for DnsConfig<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("DnsConfig").field(&self.inner).finish()
    }
}

impl<T> Transport for DnsConfig<T>
where
    T: Transport + Send + 'static,
    T::Error: Send,
    T::Dial: Send
{
    type Output = T::Output;
    type Error = DnsErr<T::Error>;
    type Listener = stream::MapErr<
        stream::MapOk<T::Listener,
            fn(ListenerEvent<T::ListenerUpgrade, T::Error>) -> ListenerEvent<Self::ListenerUpgrade, Self::Error>>,
        fn(T::Error) -> Self::Error>;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, fn(T::Error) -> Self::Error>;
    type Dial = future::Either<
        future::MapErr<T::Dial, fn(T::Error) -> Self::Error>,
        BoxFuture<'static, Result<Self::Output, Self::Error>>
    >;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let listener = self.inner.listen_on(addr).map_err(|err| err.map(DnsErr::Underlying))?;
        let listener = listener
            .map_ok::<_, fn(_) -> _>(|event| {
                event
                    .map(|upgr| {
                        upgr.map_err::<_, fn(_) -> _>(DnsErr::Underlying)
                    })
                    .map_err(DnsErr::Underlying)
            })
            .map_err::<_, fn(_) -> _>(DnsErr::Underlying);
        Ok(listener)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // As an optimization, we immediately pass through if no component of the address contain
        // a DNS protocol.
        let contains_dns = addr.iter().any(|cmp| match cmp {
            Protocol::Dns(_) => true,
            Protocol::Dns4(_) => true,
            Protocol::Dns6(_) => true,
            _ => false,
        });

        if !contains_dns {
            trace!("Pass-through address without DNS: {}", addr);
            let inner_dial = self.inner.dial(addr)
                .map_err(|err| err.map(DnsErr::Underlying))?;
            return Ok(inner_dial.map_err::<_, fn(_) -> _>(DnsErr::Underlying).left_future());
        }

        trace!("Dialing address with DNS: {}", addr);
        let resolve_futs = addr.iter()
            .map(|cmp| match cmp {
                Protocol::Dns(ref name) | Protocol::Dns4(ref name) | Protocol::Dns6(ref name) => {
                    let name = name.to_string();
                    let to_resolve = format!("{}:0", name);
                    let (tx, rx) = oneshot::channel();
                    self.thread_pool.spawn_ok(async {
                        let to_resolve = to_resolve;
                        let _ = tx.send(match to_resolve[..].to_socket_addrs() {
                            Ok(list) => Ok(list.map(|s| s.ip()).collect::<Vec<_>>()),
                            Err(e) => Err(e),
                        });
                    });

                    let (dns4, dns6) = match cmp {
                        Protocol::Dns(_) => (true, true),
                        Protocol::Dns4(_) => (true, false),
                        Protocol::Dns6(_) => (false, true),
                        _ => unreachable!(),
                    };

                    async move {
                        let list = rx.await
                            .map_err(|_| {
                                error!("DNS resolver crashed");
                                DnsErr::ResolveFail(name.clone())
                            })?
                            .map_err(|err| DnsErr::ResolveError {
                                domain_name: name.clone(),
                                error: err,
                            })?;

                        list.into_iter()
                            .filter_map(|addr| {
                                if (dns4 && addr.is_ipv4()) || (dns6 && addr.is_ipv6()) {
                                    Some(Protocol::from(addr))
                                } else {
                                    None
                                }
                            })
                            .next()
                            .ok_or_else(|| DnsErr::ResolveFail(name))
                    }.left_future()
                },
                cmp => future::ready(Ok(cmp.acquire())).right_future()
            })
            .collect::<stream::FuturesOrdered<_>>();

        let future = resolve_futs.collect::<Vec<_>>()
            .then(move |outcome| async move {
                let outcome = outcome.into_iter().collect::<Result<Vec<_>, _>>()?;
                let outcome = outcome.into_iter().collect::<Multiaddr>();
                debug!("DNS resolution outcome: {} => {}", addr, outcome);

                match self.inner.dial(outcome) {
                    Ok(d) => d.await.map_err(DnsErr::Underlying),
                    Err(TransportError::MultiaddrNotSupported(_addr)) =>
                        Err(DnsErr::MultiaddrNotSupported),
                    Err(TransportError::Other(err)) => Err(DnsErr::Underlying(err))
                }
            });

        Ok(future.boxed().right_future())
    }
}

/// Error that can be generated by the DNS layer.
#[derive(Debug)]
pub enum DnsErr<TErr> {
    /// Error in the underlying transport layer.
    Underlying(TErr),
    /// Failed to find any IP address for this DNS address.
    ResolveFail(String),
    /// Error while resolving a DNS address.
    ResolveError {
        domain_name: String,
        error: io::Error,
    },
    /// Found an IP address, but the underlying transport doesn't support the multiaddr.
    MultiaddrNotSupported,
}

impl<TErr> fmt::Display for DnsErr<TErr>
where TErr: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsErr::Underlying(err) => write!(f, "{}", err),
            DnsErr::ResolveFail(addr) => write!(f, "Failed to resolve DNS address: {:?}", addr),
            DnsErr::ResolveError { domain_name, error } => {
                write!(f, "Failed to resolve DNS address: {:?}; {:?}", domain_name, error)
            },
            DnsErr::MultiaddrNotSupported => write!(f, "Resolve multiaddr not supported"),
        }
    }
}

impl<TErr> error::Error for DnsErr<TErr>
where TErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DnsErr::Underlying(err) => Some(err),
            DnsErr::ResolveFail(_) => None,
            DnsErr::ResolveError { error, .. } => Some(error),
            DnsErr::MultiaddrNotSupported => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DnsConfig;
    use futures::{future::BoxFuture, prelude::*, stream::BoxStream};
    use libp2p_core::{
        Transport,
        multiaddr::{Protocol, Multiaddr},
        transport::ListenerEvent,
        transport::TransportError,
    };

    #[test]
    fn basic_resolve() {
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
        }

        futures::executor::block_on(async move {
            let transport = DnsConfig::new(CustomTransport).unwrap();

            let _ = transport
                .clone()
                .dial("/dns4/example.com/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            let _ = transport
                .clone()
                .dial("/dns6/example.com/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            let _ = transport
                .dial("/ip4/1.2.3.4/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();
        });
    }
}
