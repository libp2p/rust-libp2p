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

use futures::{future::{self, Either, FutureResult, JoinAll}, prelude::*, try_ready};
use log::Level;
use multiaddr::{Protocol, Multiaddr};
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
    T: Transport
{
    type Output = T::Output;
    type Listener = T::Listener;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = Either<T::Dial,
        DialFuture<T, JoinFuture<JoinAll<std::vec::IntoIter<Either<
            ResolveFuture<tokio_dns::IoFuture<Vec<IpAddr>>>,
            FutureResult<Protocol<'static>, IoError>>>>
        >>
    >;

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
            Protocol::Dns4(_) => true,
            Protocol::Dns6(_) => true,
            _ => false,
        });

        if !contains_dns {
            trace!("Pass-through address without DNS: {}", addr);
            return match self.inner.dial(addr) {
                Ok(d) => Ok(Either::A(d)),
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
                Protocol::Dns4(ref name) =>
                    Either::A(ResolveFuture {
                        name: if log_enabled!(Level::Trace) {
                            Some(name.clone().into_owned())
                        } else {
                            None
                        },
                        inner: resolver.resolve(name),
                        ty: ResolveTy::Dns4
                    }),
                Protocol::Dns6(ref name) =>
                    Either::A(ResolveFuture {
                        name: if log_enabled!(Level::Trace) {
                            Some(name.clone().into_owned())
                        } else {
                            None
                        },
                        inner: resolver.resolve(name),
                        ty: ResolveTy::Dns6
                    }),
                cmp => Either::B(future::ok(cmp.acquire()))
            })
            .collect::<Vec<_>>()
            .into_iter();

        let new_addr = JoinFuture { addr, future: future::join_all(resolve_iters) };
        Ok(Either::B(DialFuture { trans: Some(self.inner), future: Either::A(new_addr) }))
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        // Since `listen_on` doesn't perform any resolution, we just pass through `nat_traversal`
        // as well.
        self.inner.nat_traversal(server, observed)
    }
}

// How to resolve; to an IPv4 address or an IPv6 address?
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ResolveTy {
    Dns4,
    Dns6,
}

/// Future, performing DNS resolution.
#[derive(Debug)]
pub struct ResolveFuture<T> {
    name: Option<String>,
    inner: T,
    ty: ResolveTy
}

impl<T> Future for ResolveFuture<T>
where
    T: Future<Item = Vec<IpAddr>, Error = IoError>
{
    type Item = Protocol<'static>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let ty = self.ty;
        let addrs = try_ready!(self.inner.poll());
        trace!("DNS component resolution: {:?} => {:?}", self.name, addrs);
        let mut addrs = addrs
            .into_iter()
            .filter_map(move |addr| match (addr, ty) {
                (IpAddr::V4(addr), ResolveTy::Dns4) => Some(Protocol::Ip4(addr)),
                (IpAddr::V6(addr), ResolveTy::Dns6) => Some(Protocol::Ip6(addr)),
                _ => None,
            });
        match addrs.next() {
            Some(a) => Ok(Async::Ready(a)),
            None => Err(IoError::new(IoErrorKind::Other, "couldn't find any relevant IP address"))
        }
    }
}

/// Build final multi-address from resolving futures.
#[derive(Debug)]
pub struct JoinFuture<T> {
    addr: Multiaddr,
    future: T
}

impl<T> Future for JoinFuture<T>
where
    T: Future<Item = Vec<Protocol<'static>>, Error = IoError>
{
    type Item = Multiaddr;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let outcome = try_ready!(self.future.poll());
        let outcome: Multiaddr = outcome.into_iter().collect();
        debug!("DNS resolution outcome: {} => {}", self.addr, outcome);
        Ok(Async::Ready(outcome))
    }
}

/// Future, dialing the resolved multi-address.
#[derive(Debug)]
pub struct DialFuture<T: Transport, F> {
    trans: Option<T>,
    future: Either<F, T::Dial>,
}

impl<T, F> Future for DialFuture<T, F>
where
    T: Transport,
    F: Future<Item = Multiaddr, Error = IoError>
{
    type Item = T::Output;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let next = match self.future {
                Either::A(ref mut f) => {
                    let addr = try_ready!(f.poll());
                    match self.trans.take().unwrap().dial(addr) {
                        Ok(dial) => Either::B(dial),
                        Err(_) => return Err(IoError::new(IoErrorKind::Other, "multiaddr not supported"))
                    }
                }
                Either::B(ref mut f) => return f.poll()
            };
            self.future = next
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp;
    use self::libp2p_tcp::TcpConfig;
    use futures::future;
    use swarm::Transport;
    use multiaddr::{Protocol, Multiaddr};
    use std::io::Error as IoError;
    use DnsConfig;

    #[test]
    fn basic_resolve() {
        #[derive(Clone)]
        struct CustomTransport;
        impl Transport for CustomTransport {
            type Output = <TcpConfig as Transport>::Output;
            type Listener = <TcpConfig as Transport>::Listener;
            type ListenerUpgrade = <TcpConfig as Transport>::ListenerUpgrade;
            type Dial = future::Empty<Self::Output, IoError>;

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
                    Protocol::Tcp(_) => (),
                    _ => panic!(),
                };
                match addr[0] {
                    Protocol::Dns4(_) => (),
                    Protocol::Dns6(_) => (),
                    _ => panic!(),
                };
                Ok(future::empty())
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
