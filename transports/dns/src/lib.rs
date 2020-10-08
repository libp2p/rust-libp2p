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
use async_std_resolver::{AsyncStdResolver, ResolveError, resolver_from_system_conf};
use futures::{prelude::*, future, stream};
use libp2p_core::multiaddr::{Protocol, Multiaddr};
use libp2p_core::transport::{Dialer, ListenerEvent, Transport, TransportError};
use std::{error, fmt, io, net::IpAddr, pin::Pin};

#[derive(Clone, Debug)]
pub struct Resolver {
    inner: AsyncStdResolver,
}

impl Resolver {
    pub async fn from_system_conf() -> Result<Self, ResolveError> {
        Ok(Self { inner: resolver_from_system_conf().await? })
    }

    pub async fn resolve(&self, addr: Multiaddr) -> Result<Multiaddr, DnsError> {
        // As an optimization, we immediately pass through if no component of the address contain
        // a DNS protocol.
        let contains_dns = addr.iter().any(|cmp| match cmp {
            Protocol::Dns(_) => true,
            Protocol::Dns4(_) => true,
            Protocol::Dns6(_) => true,
            _ => false,
        });

        if !contains_dns {
            log::trace!("Pass-through address without DNS: {}", addr);
            return Ok(addr);
        }

        struct Wrapper(Protocol<'static>);

        log::trace!("Dialing address with DNS: {}", addr);
        let mut resolve_futs = addr.iter()
            .map(|cmp| match cmp {
                Protocol::Dns(ref name) | Protocol::Dns4(ref name) | Protocol::Dns6(ref name) => {
                    let name = name.to_string();
                    // Appending a dot makes it a FQDN allowing for faster queries.
                    let mut domain = String::with_capacity(name.len() + 1);
                    domain.push_str(&name);
                    domain.push('.');

                    async move {
                         let addr = match cmp {
                            Protocol::Dns(_) => {
                                self.inner.lookup_ip(domain).await
                                    .map(|lookup| lookup.into_iter().next())
                            }
                            Protocol::Dns4(_) => {
                                self.inner.ipv4_lookup(domain).await
                                    .map(|lookup| lookup.into_iter().next().map(IpAddr::V4))
                            }
                            Protocol::Dns6(_) => {
                                self.inner.ipv6_lookup(domain).await
                                    .map(|lookup| lookup.into_iter().next().map(IpAddr::V6))
                            }
                            _ => unreachable!(),
                        }.map_err(|e| {
                            DnsError::ResolveError {
                                domain_name: name.clone(),
                                error: io::Error::new(io::ErrorKind::Other, e),
                            }
                        })?;
                        if let Some(addr) = addr {
                            Ok(Wrapper(Protocol::from(addr)))
                        } else {
                            Err(DnsError::ResolveFail(name))
                        }
                    }.left_future()
                },
                cmp => future::ready(Ok(Wrapper(cmp.acquire()))).right_future()
            })
            .collect::<stream::FuturesOrdered<_>>();

        let mut outcome = Multiaddr::with_capacity(addr.len());
        while let Some(res) = resolve_futs.next().await {
            let Wrapper(proto) = res?;
            outcome.push(proto);
        }
        log::debug!("DNS resolution outcome: {} => {}", addr, outcome);
        Ok(outcome)
    }
}

/// Error that can be generated by the DNS layer.
#[derive(Debug)]
pub enum DnsError {
    /// Failed to find any IP address for this DNS address.
    ResolveFail(String),
    /// Error while resolving a DNS address.
    ResolveError {
        domain_name: String,
        error: io::Error,
    },
}

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsError::ResolveFail(addr) => write!(f, "Failed to resolve DNS address: {:?}", addr),
            DnsError::ResolveError { domain_name, error } => {
                write!(f, "Failed to resolve DNS address: {:?}; {:?}", domain_name, error)
            },
        }
    }
}

impl error::Error for DnsError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DnsError::ResolveFail(_) => None,
            DnsError::ResolveError { error, .. } => Some(error),
        }
    }
}

/// Error that can be generated by the DNS layer.
#[derive(Debug)]
pub enum DnsErr<TErr> {
    /// Dns error.
    Dns(DnsError),
    /// Error in the underlying transport layer.
    Underlying(TErr),
    /// Multiaddr not supported.
    MultiaddrNotSupported(Multiaddr),
}

impl<TErr: fmt::Display> fmt::Display for DnsErr<TErr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsErr::Dns(err) => write!(f, "{}", err),
            DnsErr::Underlying(err) => write!(f, "{}", err),
            DnsErr::MultiaddrNotSupported(addr) => write!(f, "Multiaddr {} not supported", addr),
        }
    }
}

impl<TErr: error::Error + 'static> error::Error for DnsErr<TErr> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            DnsErr::Dns(err) => Some(err),
            DnsErr::Underlying(err) => Some(err),
            DnsErr::MultiaddrNotSupported(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DnsConfig<T> {
    inner: T,
    resolver: Resolver,
}

impl<T> DnsConfig<T> {
    pub fn new(inner: T, resolver: Resolver) -> Result<DnsConfig<T>, io::Error> {
        Ok(DnsConfig { inner, resolver })
    }
}

#[derive(Clone, Debug)]
pub struct DnsDialer<D> {
    dialer: D,
    resolver: Resolver,
}

impl<T> Dialer for DnsDialer<T>
where
    T: Dialer + Send + 'static,
    T::Dial: Send + 'static,
    T::Error: Send + 'static,
{
    type Output = T::Output;
    type Error = DnsErr<T::Error>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        Ok(Box::pin(async move {
            let addr = self.resolver.resolve(addr).await.map_err(|e| DnsErr::Dns(e))?;
            let out = self.dialer.dial(addr)
                .map_err(|e| match e {
                    TransportError::Other(e) => DnsErr::Underlying(e),
                    TransportError::MultiaddrNotSupported(addr) => DnsErr::MultiaddrNotSupported(addr),
                })?
                .await
                .map_err(|e| DnsErr::Underlying(e))?;
            Ok(out)
        }))
    }
}

impl<T> Transport for DnsConfig<T>
where
    T: Transport + Send + 'static,
    T::Dial: Send + 'static,
    T::Dialer: Send + 'static,
    T::Error: Send + 'static,
{
    type Output = T::Output;
    type Error = DnsErr<T::Error>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Dialer = DnsDialer<T::Dialer>;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, fn(T::Error) -> Self::Error>;
    type Listener = stream::MapErr<
        stream::MapOk<T::Listener,
            fn(ListenerEvent<T::ListenerUpgrade, T::Error>) -> ListenerEvent<Self::ListenerUpgrade, Self::Error>>,
            fn(T::Error) -> Self::Error>;

    fn dialer(&self) -> Self::Dialer {
        DnsDialer { dialer: self.inner.dialer(), resolver: self.resolver.clone() }
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Self::Dialer), TransportError<Self::Error>> {
        let (listener, dialer) = match self.inner.listen_on(addr) {
            Ok(res) => res,
            Err(TransportError::Other(e)) => {
                return Err(TransportError::Other(DnsErr::Underlying(e)));
            }
            Err(TransportError::MultiaddrNotSupported(addr)) => {
                return Err(TransportError::MultiaddrNotSupported(addr));
            }
        };
        let listener = listener
            .map_ok::<_, fn(_) -> _>(|event| {
                event
                    .map(|upgr| {
                        upgr.map_err::<_, fn(_) -> _>(DnsErr::Underlying)
                    })
                    .map_err(DnsErr::Underlying)
            })
            .map_err::<_, fn(_) -> _>(DnsErr::Underlying);
        let dialer = DnsDialer { dialer, resolver: self.resolver.clone() };
        Ok((listener, dialer))
    }
}

#[cfg(test)]
mod tests {
    use super::{DnsConfig, Resolver};
    use futures::{future::BoxFuture, prelude::*};
    use libp2p_core::{
        Dialer, Transport,
        multiaddr::{Protocol, Multiaddr},
        transport::ListenerEvent,
        transport::TransportError,
    };
    use std::{pin::Pin, task::{Context, Poll}};

    #[test]
    fn basic_resolve() {
        env_logger::try_init().ok();

        #[derive(Clone)]
        struct CustomTransport;

        impl Transport for CustomTransport {
            type Output = ();
            type Error = std::io::Error;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;
            type Dialer = CustomDialer;
            type Listener = CustomListener;
            type ListenerUpgrade = ListenerUpgrade;

            fn dialer(&self) -> Self::Dialer {
                CustomDialer
            }

            fn listen_on(self, _: Multiaddr) -> Result<(Self::Listener, Self::Dialer), TransportError<Self::Error>> {
                unreachable!()
            }
        }

        struct CustomDialer;

        impl Dialer for CustomDialer {
            type Output = ();
            type Error = std::io::Error;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

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

        type ListenerUpgrade = BoxFuture<'static, Result<(), std::io::Error>>;

        struct CustomListener;

        impl Stream for CustomListener {
            type Item = Result<ListenerEvent<ListenerUpgrade, std::io::Error>, std::io::Error>;

            fn poll_next(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Option<Self::Item>> {
                unreachable!()
            }
        }

        futures::executor::block_on(async move {
            let resolver = Resolver::from_system_conf().await.unwrap();
            let transport = DnsConfig::new(CustomTransport, resolver).unwrap();

            let _ = transport
                .dialer()
                .dial("/dns4/example.com/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            let _ = transport
                .dialer()
                .dial("/dns6/example.com/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            let _ = transport
                .dialer()
                .dial("/dns/example.com/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();

            let _ = transport
                .dialer()
                .dial("/ip4/1.2.3.4/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();
        });
    }
}
