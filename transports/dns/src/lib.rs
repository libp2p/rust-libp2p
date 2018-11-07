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
extern crate libp2p_core;
extern crate log;
extern crate multiaddr;
extern crate tokio_dns;
extern crate tokio_io;

use futures::future::{self, Future};
use log::{Level, debug, log_enabled, trace};
use multiaddr::{Protocol, Multiaddr};
use std::{fmt, io, net::IpAddr};
use libp2p_core::transport::Dialer;
use tokio_dns::{CpuPoolResolver, Resolver};

/// Represents the configuration for a DNS dialer capability of libp2p.
///
/// This struct implements the `Dialer` trait and holds an underlying dialer. Any call to
/// `dial` with a multiaddr that contains `/dns4/` or `/dns6/` will be first be resolved, then
/// passed to the underlying transport.
#[derive(Clone)]
pub struct DnsDialer<D> {
    dialer: D,
    resolver: CpuPoolResolver,
}

impl<D> DnsDialer<D> {
    /// Creates a new configuration object for DNS.
    #[inline]
    pub fn new(dialer: D) -> Self {
        DnsDialer::with_resolve_threads(dialer, 1)
    }

    /// Same as `new`, but allows specifying a number of threads for the resolving.
    #[inline]
    pub fn with_resolve_threads(dialer: D, num_threads: usize) -> Self {
        trace!("Created a CpuPoolResolver");
        DnsDialer { dialer, resolver: CpuPoolResolver::new(num_threads) }
    }
}

impl<D> fmt::Debug for DnsDialer<D>
where
    D: fmt::Debug,
{
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_tuple("DnsDialer").field(&self.dialer).finish()
    }
}

impl<D> Dialer for DnsDialer<D>
where
    D: Dialer + Send + 'static,
    D::Outbound: Send + 'static,
    D::Error: From<io::Error>
{
    type Output = D::Output;
    type Error = D::Error;
    type Outbound = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        let contains_dns = addr.iter().any(|cmp| match cmp {
            Protocol::Dns4(_) => true,
            Protocol::Dns6(_) => true,
            _ => false,
        });

        if !contains_dns {
            trace!("Pass-through address without DNS: {}", addr);
            return match self.dialer.dial(addr) {
                Ok(d) => Ok(Box::new(d) as Box<_>),
                Err((dialer, addr)) => Err((DnsDialer { dialer, resolver: self.resolver }, addr)),
            };
        }

        let resolver = self.resolver;

        trace!("Dialing address with DNS: {}", addr);
        let resolve_iters = addr.iter()
            .map(move |cmp| match cmp {
                Protocol::Dns4(ref name) => {
                    future::Either::A(resolve_dns(name, &resolver, ResolveTy::Dns4))
                }
                Protocol::Dns6(ref name) => {
                    future::Either::A(resolve_dns(name, &resolver, ResolveTy::Dns6))
                }
                cmp => future::Either::B(future::ok(cmp.acquire())),
            })
            .collect::<Vec<_>>()
            .into_iter();

        let new_addr = future::join_all(resolve_iters).map(move |outcome| {
            let outcome: Multiaddr = outcome.into_iter().collect();
            debug!("DNS resolution outcome: {} => {}", addr, outcome);
            outcome
        });

        let dialer = self.dialer;
        let future = new_addr
            .and_then(move |addr| {
                dialer
                    .dial(addr)
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "multiaddr not supported"))
            })
            .flatten();

        Ok(Box::new(future) as Box<_>)
    }
}

// How to resolve; to an IPv4 address or an IPv6 address?
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ResolveTy {
    Dns4,
    Dns6,
}

// Resolve a DNS name and returns a future with the result.
fn resolve_dns<'a>(name: &str, resolver: &CpuPoolResolver, ty: ResolveTy)
    -> impl Future<Item = Protocol<'a>, Error = io::Error>
{
    let debug_name = if log_enabled!(Level::Trace) {
        Some(name.to_owned())
    } else {
        None
    };

    resolver.resolve(name).and_then(move |addrs| {
        if log_enabled!(Level::Trace) {
            trace!(
                "DNS component resolution: {} => {:?}",
                debug_name.expect("trace log level was enabled"),
                addrs
            );
        }

        addrs
            .into_iter()
            .filter_map(move |addr| match (addr, ty) {
                (IpAddr::V4(addr), ResolveTy::Dns4) => Some(Protocol::Ip4(addr)),
                (IpAddr::V6(addr), ResolveTy::Dns6) => Some(Protocol::Ip6(addr)),
                _ => None,
            })
            .next()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::Other, "couldn't find any relevant IP address")
            })
    })
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    use self::libp2p_tcp_transport::TcpConfig;
    use futures::future;
    use libp2p_core::transport::Dialer;
    use multiaddr::{Protocol, Multiaddr};
    use super::DnsDialer;

    #[test]
    fn basic_resolve() {
        #[derive(Clone)]
        struct CustomDialer;

        impl Dialer for CustomDialer {
            type Output = <TcpConfig as Dialer>::Output;
            type Error = <TcpConfig as Dialer>::Error;
            type Outbound = future::Empty<Self::Output, Self::Error>;

            fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
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
        }

        let dialer = DnsDialer::new(CustomDialer);

        let _ = dialer
            .clone()
            .dial("/dns4/example.com/tcp/20000".parse().unwrap())
            .unwrap_or_else(|_| panic!());

        let _ = dialer
            .dial("/dns6/example.com/tcp/20000".parse().unwrap())
            .unwrap_or_else(|_| panic!());
    }
}
