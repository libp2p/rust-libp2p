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

use futures::{future::{self, Either, FutureResult, JoinAll}, prelude::*, stream, try_ready};
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{TransportError, ListenerEvent}
};
use log::{debug, trace, log_enabled, Level};
use std::{error, fmt, io, marker::PhantomData, net::IpAddr};
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
    pub fn new(inner: T) -> DnsConfig<T> {
        DnsConfig::with_resolve_threads(inner, 1)
    }

    /// Same as `new`, but allows specifying a number of threads for the resolving.
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
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("DnsConfig").field(&self.inner).finish()
    }
}

impl<T> Transport for DnsConfig<T>
where
    T: Transport,
    T::Error: 'static,
{
    type Output = T::Output;
    type Error = DnsErr<T::Error>;
    type Listener = stream::MapErr<
        stream::Map<T::Listener,
            fn(ListenerEvent<T::ListenerUpgrade>) -> ListenerEvent<Self::ListenerUpgrade>>,
        fn(T::Error) -> Self::Error>;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, fn(T::Error) -> Self::Error>;
    type Dial = Either<future::MapErr<T::Dial, fn(T::Error) -> Self::Error>,
        DialFuture<T, JoinFuture<JoinAll<std::vec::IntoIter<Either<
            ResolveFuture<tokio_dns::IoFuture<Vec<IpAddr>>, T::Error>,
            FutureResult<Protocol<'static>, Self::Error>>>>
        >>
    >;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let listener = self.inner.listen_on(addr).map_err(|err| err.map(DnsErr::Underlying))?;
        let listener = listener
            .map::<_, fn(_) -> _>(|event| event.map(|upgr| {
                upgr.map_err::<fn(_) -> _, _>(DnsErr::Underlying)
            }))
            .map_err::<_, fn(_) -> _>(DnsErr::Underlying);
        Ok(listener)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let contains_dns = addr.iter().any(|cmp| match cmp {
            Protocol::Dns4(_) => true,
            Protocol::Dns6(_) => true,
            _ => false,
        });

        if !contains_dns {
            trace!("Pass-through address without DNS: {}", addr);
            let inner_dial = self.inner.dial(addr).map_err(|err| err.map(DnsErr::Underlying))?;
            return Ok(Either::A(inner_dial.map_err(DnsErr::Underlying)));
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
                        ty: ResolveTy::Dns4,
                        error_ty: PhantomData,
                    }),
                Protocol::Dns6(ref name) =>
                    Either::A(ResolveFuture {
                        name: if log_enabled!(Level::Trace) {
                            Some(name.clone().into_owned())
                        } else {
                            None
                        },
                        inner: resolver.resolve(name),
                        ty: ResolveTy::Dns6,
                        error_ty: PhantomData,
                    }),
                cmp => Either::B(future::ok(cmp.acquire()))
            })
            .collect::<Vec<_>>()
            .into_iter();

        let new_addr = JoinFuture { addr, future: future::join_all(resolve_iters) };
        Ok(Either::B(DialFuture { trans: Some(self.inner), future: Either::A(new_addr) }))
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

// How to resolve; to an IPv4 address or an IPv6 address?
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ResolveTy {
    Dns4,
    Dns6,
}

/// Future, performing DNS resolution.
#[derive(Debug)]
pub struct ResolveFuture<T, E> {
    name: Option<String>,
    inner: T,
    ty: ResolveTy,
    error_ty: PhantomData<E>,
}

impl<T, E> Future for ResolveFuture<T, E>
where
    T: Future<Item = Vec<IpAddr>, Error = io::Error>
{
    type Item = Protocol<'static>;
    type Error = DnsErr<E>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let ty = self.ty;
        let addrs = try_ready!(self.inner.poll().map_err(|error| {
            let domain_name = self.name.take().unwrap_or_default();
            DnsErr::ResolveError { domain_name, error }
        }));

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
            None => Err(DnsErr::ResolveFail(self.name.take().unwrap_or_default()))
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
    T: Future<Item = Vec<Protocol<'static>>>
{
    type Item = Multiaddr;
    type Error = T::Error;

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

impl<T, F, TErr> Future for DialFuture<T, F>
where
    T: Transport<Error = TErr>,
    F: Future<Item = Multiaddr, Error = DnsErr<TErr>>,
    TErr: error::Error,
{
    type Item = T::Output;
    type Error = DnsErr<TErr>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let next = match self.future {
                Either::A(ref mut f) => {
                    let addr = try_ready!(f.poll());
                    match self.trans.take().unwrap().dial(addr) {
                        Ok(dial) => Either::B(dial),
                        Err(_) => return Err(DnsErr::MultiaddrNotSupported)
                    }
                }
                Either::B(ref mut f) => return f.poll().map_err(DnsErr::Underlying)
            };
            self.future = next
        }
    }
}

#[cfg(test)]
mod tests {
    use libp2p_tcp::TcpConfig;
    use futures::future;
    use libp2p_core::{
        Transport,
        multiaddr::{Protocol, Multiaddr},
        transport::TransportError
    };
    use super::DnsConfig;

    #[test]
    fn basic_resolve() {
        #[derive(Clone)]
        struct CustomTransport;

        impl Transport for CustomTransport {
            type Output = <TcpConfig as Transport>::Output;
            type Error = <TcpConfig as Transport>::Error;
            type Listener = <TcpConfig as Transport>::Listener;
            type ListenerUpgrade = <TcpConfig as Transport>::ListenerUpgrade;
            type Dial = future::Empty<Self::Output, Self::Error>;

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
                    Protocol::Dns4(_) => (),
                    Protocol::Dns6(_) => (),
                    _ => panic!(),
                };
                Ok(future::empty())
            }
        }

        let transport = DnsConfig::new(CustomTransport);

        let _ = transport
            .clone()
            .dial("/dns4/example.com/tcp/20000".parse().unwrap())
            .unwrap();
        let _ = transport
            .dial("/dns6/example.com/tcp/20000".parse().unwrap())
            .unwrap();
    }
}
