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

// TODO: use this once stable ; for now we just copy-paste the content of the README.md
//#![doc(include = "../README.md")]

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
//! `/dns4/` or `/dns6/` component, a DNS resolve will be performed and the component will be
//! replaced with respectively an `/ip4/` or an `/ip6/` component.
//!

extern crate futures;
extern crate libp2p_core as swarm;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate tokio_dns;
extern crate tokio_io;

use futures::future::{self, Future};
use log::Level;
use multiaddr::{AddrComponent, Multiaddr};
use std::fmt;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::net::IpAddr;
use swarm::Transport;
use tokio_dns::{CpuPoolResolver, Resolver};

/// Represents the configuration for a DNS transport capability of libp2p.
///
/// This struct implements the `Transport` trait and holds an underlying transport. Any call to
/// `dial` with a multiaddr that contains `/dns4/` or `/dns6/` will be first be resolved, then
/// passed to the underlying transport.
///
/// Listening is unaffected.
#[derive(Clone)]
pub struct DnsConfig<T> {
    inner: T,
    resolver: CpuPoolResolver,
}

impl<T> DnsConfig<T> {
    /// Creates a new configuration object for DNS.
    #[inline]
    pub fn new(inner: T) -> DnsConfig<T> {
        DnsConfig::with_resolve_threads(inner, 1)
    }

    /// Same as `new`, but allows specifying a number of threads for the resolving.
    #[inline]
    pub fn with_resolve_threads(inner: T, num_threads: usize) -> DnsConfig<T> {
        trace!("Created a CpuPoolResolver");

        DnsConfig {
            inner,
            resolver: CpuPoolResolver::new(num_threads),
        }
    }
}

impl<T> fmt::Debug for DnsConfig<T>
where
    T: fmt::Debug,
{
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_tuple("DnsConfig").field(&self.inner).finish()
    }
}

impl<T> Transport for DnsConfig<T>
where
    T: Transport + 'static, // TODO: 'static :-/
{
    type Output = T::Output;
    type Listener = T::Listener;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        match self.inner.listen_on(addr) {
            Ok(r) => Ok(r),
            Err((inner, addr)) => Err((
                DnsConfig {
                    inner,
                    resolver: self.resolver,
                },
                addr,
            )),
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let contains_dns = addr.iter().any(|cmp| match cmp {
            AddrComponent::DNS4(_) => true,
            AddrComponent::DNS6(_) => true,
            _ => false,
        });

        if !contains_dns {
            trace!("Pass-through address without DNS: {}", addr);
            return match self.inner.dial(addr) {
                Ok(d) => Ok(Box::new(d) as Box<_>),
                Err((inner, addr)) => Err((
                    DnsConfig {
                        inner,
                        resolver: self.resolver,
                    },
                    addr,
                )),
            };
        }

        let resolver = self.resolver;

        trace!("Dialing address with DNS: {}", addr);
        let resolve_iters = addr.iter()
            .map(move |cmp| match cmp {
                AddrComponent::DNS4(ref name) => {
                    future::Either::A(resolve_dns(name, resolver.clone(), ResolveTy::Dns4))
                }
                AddrComponent::DNS6(ref name) => {
                    future::Either::A(resolve_dns(name, resolver.clone(), ResolveTy::Dns6))
                }
                cmp => future::Either::B(future::ok(cmp)),
            })
            .collect::<Vec<_>>()
            .into_iter();

        let new_addr = future::join_all(resolve_iters).map(move |outcome| {
            let outcome: Multiaddr = outcome.into_iter().collect();
            debug!("DNS resolution outcome: {} => {}", addr, outcome);
            outcome
        });

        let inner = self.inner;
        let future = new_addr
            .and_then(move |addr| {
                inner
                    .dial(addr)
                    .map_err(|_| IoError::new(IoErrorKind::Other, "multiaddr not supported"))
            })
            .flatten();

        Ok(Box::new(future) as Box<_>)
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        // Since `listen_on` doesn't perform any resolution, we just pass through `nat_traversal`
        // as well.
        self.inner.nat_traversal(server, observed)
    }
}

// How to resolve ; to an IPv4 address or an IPv6 address?
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ResolveTy {
    Dns4,
    Dns6,
}

// Resolve a DNS name and returns a future with the result.
fn resolve_dns(
    name: &str,
    resolver: CpuPoolResolver,
    ty: ResolveTy,
) -> Box<Future<Item = AddrComponent, Error = IoError>> {
    let debug_name = if log_enabled!(Level::Trace) {
        Some(name.to_owned())
    } else {
        None
    };

    let future = resolver.resolve(name).and_then(move |addrs| {
        if log_enabled!(Level::Trace) {
            trace!("DNS component resolution: {} => {:?}",
                    debug_name.expect("trace log level was enabled"), addrs);
        }

        addrs
            .into_iter()
            .filter_map(move |addr| match (addr, ty) {
                (IpAddr::V4(addr), ResolveTy::Dns4) => Some(AddrComponent::IP4(addr)),
                (IpAddr::V6(addr), ResolveTy::Dns6) => Some(AddrComponent::IP6(addr)),
                _ => None,
            })
            .next()
            .ok_or(IoError::new(
                IoErrorKind::Other,
                "couldn't find any relevant IP address",
            ))
    });

    Box::new(future)
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    use self::libp2p_tcp_transport::TcpConfig;
    use DnsConfig;
    use futures::{future, Future};
    use multiaddr::{AddrComponent, Multiaddr};
    use std::io::Error as IoError;
    use swarm::Transport;

    #[test]
    fn basic_resolve() {
        #[derive(Clone)]
        struct CustomTransport;
        impl Transport for CustomTransport {
            type Output = <TcpConfig as Transport>::Output;
            type Listener = <TcpConfig as Transport>::Listener;
            type ListenerUpgrade = <TcpConfig as Transport>::ListenerUpgrade;
            type Dial = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;

            #[inline]
            fn listen_on(
                self,
                _addr: Multiaddr,
            ) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
                unreachable!()
            }

            fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
                let addr = addr.iter().collect::<Vec<_>>();
                assert_eq!(addr.len(), 2);
                match addr[1] {
                    AddrComponent::TCP(_) => (),
                    _ => panic!(),
                };
                match addr[0] {
                    AddrComponent::DNS4(_) => (),
                    AddrComponent::DNS6(_) => (),
                    _ => panic!(),
                };
                Ok(Box::new(future::empty()) as Box<_>)
            }

            #[inline]
            fn nat_traversal(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
                panic!()
            }
        }

        let transport = DnsConfig::new(CustomTransport);

        let _ = transport
            .clone()
            .dial("/dns4/example.com/tcp/20000".parse().unwrap())
            .unwrap_or_else(|_| panic!());
        let _ = transport
            .dial("/dns6/example.com/tcp/20000".parse().unwrap())
            .unwrap_or_else(|_| panic!());
    }
}
