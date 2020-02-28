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

use futures::{prelude::*, future::BoxFuture, stream::FuturesOrdered};
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{TransportError, ListenerEvent}
};
use log::{error, debug, trace};
use std::{error, fmt, io, net::ToSocketAddrs};
use std::str::FromStr;
use trust_dns_client::rr::{DNSClass, RecordType};
use trust_dns_client::udp::UdpClientConnection;
use trust_dns_client::client::{SyncClient, Client};
use trust_dns_proto::rr::domain::Name;

/// Represents the configuration for a DNS transport capability of libp2p.
///
/// This struct implements the `Transport` trait and holds an underlying transport. Any call to
/// `dial` with a multiaddr that contains `/dns4/` or `/dns6/` will be first be resolved, then
/// passed to the underlying transport.
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
            fn(ListenerEvent<T::ListenerUpgrade>) -> ListenerEvent<Self::ListenerUpgrade>>,
        fn(T::Error) -> Self::Error>;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, fn(T::Error) -> Self::Error>;
    type Dial = future::Either<
        future::MapErr<T::Dial, fn(T::Error) -> Self::Error>,
        BoxFuture<'static, Result<Self::Output, Self::Error>>
    >;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let listener = self.inner.listen_on(addr).map_err(|err| err.map(DnsErr::Underlying))?;
        let listener = listener
            .map_ok::<_, fn(_) -> _>(|event| event.map(|upgr| {
                upgr.map_err::<_, fn(_) -> _>(DnsErr::Underlying)
            }))
            .map_err::<_, fn(_) -> _>(DnsErr::Underlying);
        Ok(listener)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // As an optimization, we immediately pass through if no component of the address contain
        // a DNS protocol.
        let contains_dns = addr.iter().any(|cmp| match cmp {
            Protocol::Dns4(_) => true,
            Protocol::Dns6(_) => true,
            Protocol::Dnsaddr(_) => true,
            _ => false,
        });

        if !contains_dns {
            trace!("Pass-through address without DNS: {}", addr);
            let inner_dial = self.inner.dial(addr)
                .map_err(|err| err.map(DnsErr::Underlying))?;
            return Ok(inner_dial.map_err::<_, fn(_) -> _>(DnsErr::Underlying).left_future());
        }

        trace!("Dialing address with DNS: {}", addr);
        let mut resolve_futs = FuturesOrdered::new();
        let protocols = addr.iter().collect::<Vec<_>>();

        for (i, protocol) in addr.iter().enumerate() {
            resolve_futs.push(match protocol {
                Protocol::Dns4(_) | Protocol::Dns6(_) | Protocol::Dnsaddr(_) => {
                    let is_dnsaddr = if let Protocol::Dnsaddr(_) = protocol { true } else { false };
                    let suffix: Multiaddr = (&protocols[(i + 1)..]).into();

                    resolve_dns::<T>(protocol.clone().acquire())
                        .and_then(move |resolved_addr| {
                            if is_dnsaddr {
                                suffix_matching::<T>(resolved_addr, suffix).left_future()
                            } else {
                                future::ok(resolved_addr).right_future()
                            }
                        })
                        .left_future()
                }
                ref p => future::ready(Ok(p.clone().into())).right_future()
            });

            // dnsaddr consumes the rest of the components of the multiaddr
            if let Protocol::Dnsaddr(_) = protocol {
                break;
            }
        }

        let future = resolve_futs.collect::<Vec<_>>()
            .then(move |outcome| async move {
                let outcome = outcome.into_iter()
                    .collect::<Result<Vec<Multiaddr>, _>>()?
                    .iter()
                    .fold(Multiaddr::empty(), |mut m, next| {
                        m.concat(next);
                        m
                    });
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
    /// Found an IP address, but the suffix doesn't matched.
    SuffixDoesNotMatched,
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
            DnsErr::SuffixDoesNotMatched => write!(f, "Suffix does not matched"),
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
            DnsErr::SuffixDoesNotMatched => None,
        }
    }
}

async fn resolve_dns<T>(p: Protocol<'_>) -> Result<Multiaddr, DnsErr<T::Error>>
where
    T: Transport
{
    match p {
        Protocol::Dns4(ref name) | Protocol::Dns6(ref name) => {
            let to_resolve = format!("{}:0", name);
            let list = match to_resolve[..].to_socket_addrs() {
                Ok(list) => Ok(list.map(|s| s.ip()).collect::<Vec<_>>()),
                Err(e) => Err(e)
            }.map_err(|_| {
                error!("DNS resolver crashed");
                DnsErr::ResolveFail(name.to_string())
            })?;

            let is_dns4 = if let Protocol::Dns4(_) = p { true } else { false };

            list.into_iter()
                .filter_map(|addr| {
                    if (is_dns4 && addr.is_ipv4()) || (!is_dns4 && addr.is_ipv6()) {
                        Some(Multiaddr::from(addr))
                    } else {
                        None
                    }
                })
                .next()
                .ok_or_else(|| DnsErr::ResolveFail(name.to_string()))
        }
        Protocol::Dnsaddr(ref n) => {
            let conn = UdpClientConnection::new("8.8.8.8:53".parse().unwrap()).unwrap(); // TODO: error handling
            let client = SyncClient::new(conn); // TODO: should use async client?

            let name = Name::from_str(&format!("_dnsaddr.{}", n)).unwrap(); // TODO: error handling
            let response = client.query(&name, DNSClass::IN, RecordType::TXT).unwrap(); // TODO: error handling
            let resolved_addrs = response.answers().iter()
                .filter_map(|record| {
                    if let trust_dns_client::proto::rr::RData::TXT(ref txt) = record.rdata() {
                        Some(txt.iter())
                    } else {
                        None
                    }
                })
                .flatten()
                .filter_map(|bytes| {
                    let str = std::str::from_utf8(bytes).unwrap(); // TODO: error handling
                    let addr = Multiaddr::from_str(str.trim_start_matches("dnsaddr=")).unwrap(); // TODO: error handling

                    match addr.iter().next() {
                        Some(Protocol::Ip4(_)) | Some(Protocol::Ip6(_)) => {
                            Some(addr)
                        }
                        Some(Protocol::Dnsaddr(ref name)) => {
                            // TODO: recursive resolution
                            error!("Resolved to the dnsaddr {:?} but recursive resolution is not supported for now.", name);
                            None
                        }
                        protocol => {
                            // TODO: error handling
                            error!("{:?}", protocol);
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            // if resolved_addrs.len() == 0 {} // TODO

            Ok(resolved_addrs[0].clone())
        }
        _ => unreachable!()
    }
}

async fn suffix_matching<T>(resolved_addr: Multiaddr, suffix: Multiaddr) -> Result<Multiaddr, DnsErr<T::Error>>
where
    T: Transport
{
    let resolved_addr_len = resolved_addr.iter().count();
    let suffix_len = suffix.iter().count();

    // Make sure the addr is at least as long as the suffix we're looking for.
    if resolved_addr_len < suffix_len {
        // not long enough.
        return Err(DnsErr::SuffixDoesNotMatched);
    }

    // Matches everything after the /dnsaddr/... with the end of the dnsaddr record.
    //
    // v-------------------resolved_addr_len---------------------v
    // /ip6/2001:db8:20:3:1000:100:20:3/tcp/1234/p2p/Qmxxxxxxxxxxx
    //                                          /p2p/Qmxxxxxxxxxxx
    // ^----(resolved_addr_len - suffix_len)----^---suffix_len---^
    let resolved_addr_suffix: Multiaddr = {
        let protocols = resolved_addr.iter().collect::<Vec<Protocol>>();
        protocols[(resolved_addr_len - suffix_len)..].into()
    };

    if resolved_addr_suffix == suffix {
        Ok(resolved_addr)
    } else {
        Err(DnsErr::SuffixDoesNotMatched)
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
            type Listener = BoxStream<'static, Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>>;
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
                .clone()
                .dial("/ip4/1.2.3.4/tcp/20000".parse().unwrap())
                .unwrap()
                .await
                .unwrap();
        });
    }

    #[test]
    fn dnsaddr() {
        #[derive(Clone)]
        struct CustomTransport;

        impl Transport for CustomTransport {
            type Output = ();
            type Error = std::io::Error;
            type Listener = BoxStream<'static, Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>>;
            type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

            fn listen_on(self, _: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
                unreachable!()
            }

            fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
                let addr = addr.iter().collect::<Vec<_>>();
                assert_eq!(addr.len(), 3);
                // TODO: ipfs
//                match addr[2] {
//                    Protocol::Ipfs(_) => (),
//                    _ => panic!(),
//                };
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
                .dial("/dnsaddr/sjc-1.bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN".parse().unwrap())
                .unwrap()
                .await
                .unwrap();
        });
    }
}
