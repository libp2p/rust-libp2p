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

use fnv::FnvHashMap;
use futures::{future, Future, Stream};
use libp2p_core::{Multiaddr, MuxedTransport, Transport};
use parking_lot::Mutex;
use protocol::{IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};
use std::collections::hash_map::Entry;
use std::error::Error;
use std::io::Error as IoError;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `Transport`. See [the crate root description](index.html).
pub struct IdentifyTransport<Trans> {
    transport: Trans,
    // Each entry is protected by an asynchronous mutex, so that if we dial the same node twice
    // simultaneously, the second time will block until the first time has identified it.
    cache: Arc<Mutex<FnvHashMap<Multiaddr, CacheEntry>>>,
}

impl<Trans> Clone for IdentifyTransport<Trans>
    where Trans: Clone,
{
    fn clone(&self) -> Self {
        IdentifyTransport {
            transport: self.transport.clone(),
            cache: self.cache.clone(),
        }
    }
}

type CacheEntry = future::Shared<Box<Future<Item = IdentifyTransportOutcome, Error = IoError> + Send>>;

impl<Trans> IdentifyTransport<Trans> {
    /// Creates an `IdentifyTransport` that wraps around the given transport and peerstore.
    #[inline]
    pub fn new(transport: Trans) -> Self {
        IdentifyTransport {
            transport,
            cache: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl<Trans> Transport for IdentifyTransport<Trans>
where
    Trans: Transport + Clone + Send + 'static, // TODO: 'static :(
    Trans::Dial: Send,
    Trans::Listener: Send,
    Trans::ListenerUpgrade: Send,
    Trans::MultiaddrFuture: Send,
    Trans::Output: AsyncRead + AsyncWrite + Send,
{
    type Output = IdentifyTransportOutput<Trans::Output>;
    type MultiaddrFuture = future::FutureResult<Multiaddr, IoError>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError> + Send>;
    type ListenerUpgrade = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError> + Send>;
    type Dial = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError> + Send>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let (listener, new_addr) = match self.transport.clone().listen_on(addr.clone()) {
            Ok((l, a)) => (l, a),
            Err((inner, addr)) => {
                let id = IdentifyTransport {
                    transport: inner,
                    cache: self.cache,
                };
                return Err((id, addr));
            }
        };

        let identify_upgrade = self.transport.with_upgrade(IdentifyProtocolConfig);
        let cache = self.cache.clone();

        let listener = listener.map(move |connec| {
            let identify_upgrade = identify_upgrade.clone();
            let cache = cache.clone();
            let fut = connec
                .and_then(move |(connec, client_addr)| {
                    trace!("Incoming connection, waiting for client address");
                    client_addr.map(move |addr| (connec, addr))
                })
                .and_then(move |(connec, client_addr)| {
                    debug!("Incoming connection from {}", client_addr);

                    // Dial the address that connected to us and try upgrade with the
                    // identify protocol.
                    let info_future = cache_entry(&cache, client_addr.clone(), { let client_addr = client_addr.clone(); move || {
                        debug!("No cache entry for {}, dialing back in order to identify", client_addr);
                        future::lazy(|| { trace!("Starting identify back"); identify_upgrade
                            .dial(client_addr)
                            .unwrap_or_else(|(_, addr)| {
                                panic!("the multiaddr {} was determined to be valid earlier", addr)
                            }) })
                            .map(move |(identify, _)| {
                                let (info, observed_addr) = match identify {
                                    IdentifyOutput::RemoteInfo { info, observed_addr } => {
                                        (info, observed_addr)
                                    },
                                    _ => unreachable!(
                                        "the identify protocol guarantees that we receive \
                                        remote information when we dial a node"
                                    ),
                                };

                                debug!("Identified dialed back connection as pubkey {:?}", info.public_key);
                                IdentifyTransportOutcome {
                                    info,
                                    observed_addr,
                                }
                            })
                            .map_err(move |err| {
                                debug!("Failed to identify dialed back connection");
                                err
                            })
                    }});

                    let out = IdentifyTransportOutput {
                        socket: connec,
                        info: Box::new(info_future),
                    };

                    Ok((out, future::ok(client_addr)))
                });

                Box::new(fut) as Box<Future<Item = _, Error = _> + Send>
            });

        Ok((Box::new(listener) as Box<_>, new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        // We dial a first time the node.
        let dial = match self.transport.clone().dial(addr) {
            Ok(d) => d,
            Err((transport, addr)) => {
                let id = IdentifyTransport {
                    transport,
                    cache: self.cache,
                };
                return Err((id, addr));
            }
        };

        // Once successfully dialed, we dial again to identify.
        let identify_upgrade = self.transport.with_upgrade(IdentifyProtocolConfig);
        let cache = self.cache.clone();
        let future = dial
            .and_then(move |(connec, client_addr)| {
                trace!("Dialing successful, waiting for client address");
                client_addr.map(move |addr| (connec, addr))
            })
            .and_then(move |(socket, addr)| {
                trace!("Dialing successful ; client address is {}", addr);
                let info_future = cache_entry(&cache, addr.clone(), { let addr = addr.clone(); move || {
                    trace!("No cache entry for {} ; dialing again for identification", addr);
                    future::lazy(|| { trace!("Starting identify back"); identify_upgrade
                        .dial(addr)
                        .unwrap_or_else(|(_, addr)| {
                            panic!("the multiaddr {} was determined to be valid earlier", addr)
                        }) })
                        .map(move |(identify, _)| {
                            let (info, observed_addr) = match identify {
                                IdentifyOutput::RemoteInfo { info, observed_addr } => {
                                    (info, observed_addr)
                                }
                                _ => unreachable!(
                                    "the identify protocol guarantees that we receive \
                                        remote information when we dial a node"
                                ),
                            };

                            IdentifyTransportOutcome {
                                info,
                                observed_addr,
                            }
                        })
                }});

                let out = IdentifyTransportOutput {
                    socket: socket,
                    info: Box::new(info_future),
                };

                Ok((out, future::ok(addr)))
            });

        Ok(Box::new(future) as Box<_>)
    }

    #[inline]
    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

impl<Trans> MuxedTransport for IdentifyTransport<Trans>
where
    Trans: MuxedTransport + Clone + Send + 'static,
    Trans::Dial: Send,
    Trans::Listener: Send,
    Trans::ListenerUpgrade: Send,
    Trans::MultiaddrFuture: Send,
    Trans::Output: AsyncRead + AsyncWrite + Send,
    Trans::Incoming: Send,
    Trans::IncomingUpgrade: Send,
{
    type Incoming = Box<Future<Item = Self::IncomingUpgrade, Error = IoError> + Send>;
    type IncomingUpgrade = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError> + Send>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        let identify_upgrade = self.transport.clone().with_upgrade(IdentifyProtocolConfig);
        let cache = self.cache.clone();

        let future = self.transport.next_incoming().map(move |incoming| {
            let cache = cache.clone();
            let future = incoming
                .and_then(move |(connec, client_addr)| {
                    debug!("Incoming substream ; waiting for client address");
                    client_addr.map(move |addr| (connec, addr))
                })
                .and_then(move |(connec, client_addr)| {
                    debug!("Incoming substream from {}", client_addr);

                    // Dial the address that connected to us and try upgrade with the
                    // identify protocol.
                    let info_future = cache_entry(&cache, client_addr.clone(), { let client_addr = client_addr.clone(); move || {
                        debug!("No cache entry from {} ; dialing back to identify", client_addr);
                        future::lazy(|| { trace!("Starting identify back"); identify_upgrade
                            .dial(client_addr)
                            .unwrap_or_else(|(_, client_addr)| {
                                panic!("the multiaddr {} was determined to be valid earlier", client_addr)
                            }) })
                            .map(move |(identify, _)| {
                                let (info, observed_addr) = match identify {
                                    IdentifyOutput::RemoteInfo { info, observed_addr } => {
                                        (info, observed_addr)
                                    },
                                    _ => unreachable!(
                                        "the identify protocol guarantees that we receive \
                                        remote information when we dial a node"
                                    ),
                                };

                                debug!("Identified incoming substream as pubkey {:?}", info.public_key);
                                IdentifyTransportOutcome {
                                    info,
                                    observed_addr,
                                }
                            })
                            .map_err(move |err| {
                                debug!("Failed to identify incoming substream");
                                err
                            })
                    }});

                    let out = IdentifyTransportOutput {
                        socket: connec,
                        info: Box::new(info_future),
                    };

                    Ok((out, future::ok(client_addr)))
                });

            Box::new(future) as Box<Future<Item = _, Error = _> + Send>
        });

        Box::new(future) as Box<_>
    }
}

/// Output of the identify transport.
pub struct IdentifyTransportOutput<S> {
    /// The socket to communicate with the remote.
    pub socket: S,
    /// Outcome of the identification of the remote.
    pub info: Box<Future<Item = IdentifyTransportOutcome, Error = IoError> + Send>,
}

/// Outcome of the identification of the remote.
#[derive(Debug, Clone)]
pub struct IdentifyTransportOutcome {
    /// Identification of the remote.
    pub info: IdentifyInfo,
    /// Address the remote sees for us.
    pub observed_addr: Multiaddr,
}

fn cache_entry<F, Fut>(cache: &Mutex<FnvHashMap<Multiaddr, CacheEntry>>, addr: Multiaddr, if_no_entry: F)
    -> impl Future<Item = IdentifyTransportOutcome, Error = IoError>
where F: FnOnce() -> Fut,
      Fut: Future<Item = IdentifyTransportOutcome, Error = IoError> + Send + 'static,
{
    trace!("Looking up cache entry for {}", addr);
    let mut cache = cache.lock();
    match cache.entry(addr) {
        Entry::Occupied(entry) => {
            trace!("Cache entry found, cloning");
            future::Either::A(entry.get().clone())
        },

        Entry::Vacant(entry) => {
            trace!("No cache entry available");
            let future = (Box::new(if_no_entry()) as Box<Future<Item = _, Error = _> + Send>).shared();
            entry.insert(future.clone());
            future::Either::B(future)
        },
    }.map(|out| (*out).clone()).map_err(|err| IoError::new(err.kind(), err.description()))
}

// TODO: test that we receive back what the remote sent us
