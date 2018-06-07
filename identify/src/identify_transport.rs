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
use futures::{future, Future, IntoFuture, Stream};
use futures_mutex::Mutex as AsyncMutex;
use libp2p_core::{Multiaddr, MuxedTransport, Transport};
use parking_lot::Mutex;
use protocol::{IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `Transport`. See [the crate root description](index.html).
#[derive(Debug, Clone)]
pub struct IdentifyTransport<Trans> {
    transport: Trans,
    // Each entry is protected by an asynchronous mutex, so that if we dial the same node twice
    // simultaneously, the second time will block until the first time has identified it.
    cache: Arc<Mutex<FnvHashMap<Multiaddr, Arc<AsyncMutex<(IdentifyInfo, Multiaddr)>>>>>,
}

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
    Trans: Transport + Clone + 'static, // TODO: 'static :(
    Trans::Output: AsyncRead + AsyncWrite,
{
    type Output = IdentifyTransportOutput<Trans::Output>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;
    type Dial = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;

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

        let listener = listener.map(move |connec| {
            let identify_upgrade = identify_upgrade.clone();
            let fut = connec.and_then(move |(connec, client_addr)| {
                debug!("Incoming connection from {}, dialing back in order to identify", client_addr);

                // Dial the address that connected to us and try upgrade with the
                // identify protocol.
                let future = identify_upgrade
                    .clone()
                    .dial(client_addr.clone())
                    .map_err(move |_| {
                        IoError::new(IoErrorKind::Other, "couldn't dial back incoming node")
                    })
                    .map(move |id| (id, connec))
                    .into_future()
                    .and_then(move |(dial, connec)| dial.map(move |dial| (dial, connec)))
                    .and_then(move |((identify, addr), connec)| {
                        let (info, observed_addr) = match identify {
                            IdentifyOutput::RemoteInfo { info, observed_addr } => {
                                (info, observed_addr)
                            },
                            _ => unreachable!(
                                "the identify protocol guarantees that we receive \
                                 remote information when we dial a node"
                            ),
                        };

                        debug!("Identified {} as pubkey {:?}", addr, info.public_key);
                        let out = IdentifyTransportOutput {
                            socket: connec,
                            info: Box::new(future::ok(IdentifyTransportOutcome {
                                info,
                                observed_addr,
                            })),
                        };
                        Ok((out, addr))
                    })
                    .map_err(move |err| {
                        debug!("Failed to identify incoming {}", client_addr);
                        err
                    });
                future
            });

            Box::new(fut) as Box<Future<Item = _, Error = _>>
        });

        Ok((Box::new(listener) as Box<_>, new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let transport = self.transport;

        // TODO: use cache
        //if self.cache.lock().

        let identify_upgrade = transport.clone().with_upgrade(IdentifyProtocolConfig);

        // We dial a first time the node and upgrade it to identify.
        trace!("Dialing {} for identification", addr);
        let dial = match identify_upgrade.dial(addr) {
            Ok(d) => d,
            Err((_, addr)) => {
                let id = IdentifyTransport {
                    transport,
                    cache: self.cache,
                };
                return Err((id, addr));
            }
        };

        let future = dial.and_then(move |(identify, addr)| {
            let (info, observed_addr) = match identify {
                IdentifyOutput::RemoteInfo { info, observed_addr } => {
                    (info, observed_addr)
                }
                _ => unreachable!(
                    "the identify protocol guarantees that we receive \
                        remote information when we dial a node"
                ),
            };

            // Then dial the same node again.
            trace!("Identified {} as pubkey {:?} ; dialing again for real", addr, info.public_key);
            Ok(transport
                .dial(addr)
                .unwrap_or_else(|(_, addr)| {
                    panic!("the multiaddr {} was determined to be valid earlier", addr)
                })
                .into_future()
                .map(move |(dial, addr)| {
                    let out = IdentifyTransportOutput {
                        socket: dial,
                        info: Box::new(future::ok(IdentifyTransportOutcome {
                            info,
                            observed_addr,
                        })),
                    };

                    (out, addr)
                }))
        }).flatten();

        Ok(Box::new(future) as Box<_>)
    }

    #[inline]
    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

impl<Trans> MuxedTransport for IdentifyTransport<Trans>
where
    Trans: MuxedTransport + Clone + 'static,
    Trans::Output: AsyncRead + AsyncWrite,
{
    type Incoming = Box<Future<Item = Self::IncomingUpgrade, Error = IoError>>;
    type IncomingUpgrade = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        let identify_upgrade = self.transport.clone().with_upgrade(IdentifyProtocolConfig);

        let future = self.transport.next_incoming().map(move |incoming| {
            let future = incoming.and_then(move |(connec, client_addr)| {
                // On an incoming connection, dial back the node and upgrade to the identify
                // protocol.
                let future = identify_upgrade
                    .clone()
                    .dial(client_addr.clone())
                    .map_err(|_| {
                        IoError::new(IoErrorKind::Other, "couldn't dial back incoming node")
                    })
                    .into_future()
                    .and_then(move |dial| dial)
                    .map(move |dial| (dial, connec))
                    .and_then(move |((identify, addr), connec)| {
                        let (info, observed_addr) = match identify {
                            IdentifyOutput::RemoteInfo { info, observed_addr } => {
                                (info, observed_addr)
                            }
                            _ => unreachable!(
                                "the identify protocol guarantees that we receive remote \
                                 information when we dial a node"
                            ),
                        };

                        let out = IdentifyTransportOutput {
                            socket: connec,
                            info: Box::new(future::ok(IdentifyTransportOutcome {
                                info,
                                observed_addr,
                            })),
                        };

                        Ok((out, addr))
                    });
                future
            });

            Box::new(future) as Box<Future<Item = _, Error = _>>
        });

        Box::new(future) as Box<_>
    }
}

/// Output of the identify transport.
pub struct IdentifyTransportOutput<S> {
    /// The socket to communicate with the remote.
    pub socket: S,
    /// Outcome of the identification of the remote.
    pub info: Box<Future<Item = IdentifyTransportOutcome, Error = IoError>>,
}

/// Outcome of the identification of the remote.
#[derive(Debug, Clone)]
pub struct IdentifyTransportOutcome {
    /// Identification of the remote.
    pub info: IdentifyInfo,
    /// Address the remote sees for us.
    pub observed_addr: Multiaddr,
}

// TODO: test that we receive back what the remote sent us
