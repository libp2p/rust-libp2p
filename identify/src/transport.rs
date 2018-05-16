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

use futures::{future, stream, Future, IntoFuture, Stream};
use libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
use libp2p_core::{MuxedTransport, Transport};
use multiaddr::{AddrComponent, Multiaddr};
use protocol::{IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::ops::Deref;
use std::time::Duration;
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `Transport`. See [the crate root description](index.html).
#[derive(Debug, Clone)]
pub struct IdentifyTransport<Trans, PStoreRef> {
    transport: Trans,
    peerstore: PStoreRef,
    addr_ttl: Duration,
}

impl<Trans, PStoreRef> IdentifyTransport<Trans, PStoreRef> {
    /// Creates an `IdentifyTransport` that wraps around the given transport and peerstore.
    #[inline]
    pub fn new(transport: Trans, peerstore: PStoreRef) -> Self {
        IdentifyTransport::with_ttl(transport, peerstore, Duration::from_secs(3600))
    }

    /// Same as `new`, but allows specifying a time-to-live for the addresses gathered from
    /// remotes that connect to us.
    ///
    /// The default value is one hour.
    #[inline]
    pub fn with_ttl(transport: Trans, peerstore: PStoreRef, ttl: Duration) -> Self {
        IdentifyTransport {
            transport: transport,
            peerstore: peerstore,
            addr_ttl: ttl,
        }
    }
}

impl<Trans, PStore, PStoreRef> Transport for IdentifyTransport<Trans, PStoreRef>
where
    Trans: Transport + Clone + 'static, // TODO: 'static :(
    Trans::Output: AsyncRead + AsyncWrite,
    PStoreRef: Deref<Target = PStore> + Clone + 'static, // TODO: 'static :(
    for<'r> &'r PStore: Peerstore,
{
    type Output = IdentifyTransportOutput<Trans::Output>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;
    type Dial = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        // Note that `listen_on` expects a "regular" multiaddr (eg. `/ip/.../tcp/...`),
        // and not `/p2p/<foo>`.

        let (listener, new_addr) = match self.transport.clone().listen_on(addr.clone()) {
            Ok((l, a)) => (l, a),
            Err((inner, addr)) => {
                let id = IdentifyTransport {
                    transport: inner,
                    peerstore: self.peerstore,
                    addr_ttl: self.addr_ttl,
                };
                return Err((id, addr));
            }
        };

        let identify_upgrade = self.transport.with_upgrade(IdentifyProtocolConfig);
        let peerstore = self.peerstore;
        let addr_ttl = self.addr_ttl;

        let listener = listener.map(move |connec| {
            let peerstore = peerstore.clone();
            let identify_upgrade = identify_upgrade.clone();
            let fut = connec.and_then(move |(connec, client_addr)| {
                for peer_id in peerstore.peers() {
                    let peer = match peerstore.peer(&peer_id) {
                        Some(p) => p,
                        None => continue,
                    };

                    if peer.addrs().any(|addr| addr == client_addr) {
                        debug!(target: "libp2p-identify", "Incoming substream from {} \
                                                               identified as {:?}", client_addr,
                                                               peer_id);
                        let out = IdentifyTransportOutput { socket: connec, observed_addr: None };
                        let ret = (out, AddrComponent::P2P(peer_id.into_bytes()).into());
                        return future::Either::A(future::ok(ret));
                    }
                }

                debug!(target: "libp2p-identify", "Incoming connection from {}, dialing back \
                                                       in order to identify", client_addr);
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
                    .and_then(move |((identify, original_addr), connec)| {
                        // Compute the "real" address of the node (in the form `/p2p/...`) and
                        // add it to the peerstore.
                        let (observed, real_addr);
                        match identify {
                            IdentifyOutput::RemoteInfo { info, observed_addr } => {
                                observed = observed_addr;
                                real_addr = process_identify_info(
                                    &info,
                                    &*peerstore.clone(),
                                    original_addr.clone(),
                                    addr_ttl,
                                )?;
                            },
                            _ => unreachable!(
                                "the identify protocol guarantees that we receive \
                                 remote information when we dial a node"
                            ),
                        };

                        debug!(target: "libp2p-identify", "Identified {} as {}", original_addr,
                                                              real_addr);
                        let out = IdentifyTransportOutput { socket: connec, observed_addr: Some(observed) };
                        Ok((out, real_addr))
                    })
                    .map_err(move |err| {
                        warn!(target: "libp2p-identify", "Failed to identify incoming {}",
                                                             client_addr);
                        err
                    });
                future::Either::B(future)
            });

            Box::new(fut) as Box<Future<Item = _, Error = _>>
        });

        Ok((Box::new(listener) as Box<_>, new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match multiaddr_to_peerid(addr.clone()) {
            Ok(peer_id) => {
                // If the multiaddress is a peer ID, try each known multiaddress (taken from the
                // peerstore) one by one.
                let addrs = self.peerstore
                    .deref()
                    .peer(&peer_id)
                    .into_iter()
                    .flat_map(|peer| peer.addrs())
                    .collect::<Vec<_>>()
                    .into_iter();

                trace!(target: "libp2p-identify", "Try dialing peer ID {:?} ; {} multiaddrs \
                                                   loaded from peerstore", peer_id, addrs.len());

                let transport = self.transport;
                let future = stream::iter_ok(addrs)
                    // Try to dial each address through the transport.
                    .filter_map(move |addr| {
                        match transport.clone().dial(addr) {
                            Ok(dial) => Some(dial),
                            Err((_, addr)) => {
                                warn!(target: "libp2p-identify", "Address {} not supported by \
                                                                  underlying transport", addr);
                                None
                            },
                        }
                    })
                    .and_then(move |dial| dial)
                    // Pick the first non-failing dial result by filtering out the ones which fail.
                    .then(|res| Ok(res))
                    .filter_map(|res| res.ok())
                    .into_future()
                    .map_err(|(err, _)| err)
                    .and_then(move |(val, _)| {
                        match val {
                            Some((connec, inner_addr)) => {
                                debug!(target: "libp2p-identify", "Successfully dialed peer {:?} \
                                                                   through {}", peer_id,
                                                                   inner_addr);
                                let out = IdentifyTransportOutput { socket: connec, observed_addr: None };
                                Ok((out, inner_addr))
                            },
                            None => {
                                debug!(target: "libp2p-identify", "All multiaddresses failed when \
                                                                   dialing peer {:?}", peer_id);
                                // TODO: wrong error
                                Err(IoErrorKind::InvalidData.into())
                            },
                        }
                    })
                    // Replace the multiaddress with the one of the form `/p2p/...` or `/ipfs/...`.
                    .map(move |(socket, _inner_client_addr)| (socket, addr));

                Ok(Box::new(future) as Box<_>)
            }

            Err(addr) => {
                // If the multiaddress is something else, propagate it to the underlying transport
                // and identify the node.
                let transport = self.transport;
                let identify_upgrade = transport.clone().with_upgrade(IdentifyProtocolConfig);

                trace!(target: "libp2p-identify", "Pass through when dialing {}", addr);

                // We dial a first time the node and upgrade it to identify.
                let dial = match identify_upgrade.dial(addr) {
                    Ok(d) => d,
                    Err((_, addr)) => {
                        let id = IdentifyTransport {
                            transport,
                            peerstore: self.peerstore,
                            addr_ttl: self.addr_ttl,
                        };
                        return Err((id, addr));
                    }
                };

                let peerstore = self.peerstore;
                let addr_ttl = self.addr_ttl;

                let future = dial.and_then(move |identify| {
                    // On success, store the information in the peerstore and compute the
                    // "real" address of the node (of the form `/p2p/...`).
                    let (real_addr, old_addr, observed);
                    match identify {
                        (IdentifyOutput::RemoteInfo { info, observed_addr }, a) => {
                            old_addr = a.clone();
                            observed = observed_addr;
                            real_addr = process_identify_info(&info, &*peerstore, a, addr_ttl)?;
                        }
                        _ => unreachable!(
                            "the identify protocol guarantees that we receive \
                             remote information when we dial a node"
                        ),
                    };

                    // Then dial the same node again.
                    Ok(transport
                        .dial(old_addr)
                        .unwrap_or_else(|_| {
                            panic!("the same multiaddr was determined to be valid earlier")
                        })
                        .into_future()
                        .map(move |(dial, _wrong_addr)| {
                            let out = IdentifyTransportOutput { socket: dial, observed_addr: Some(observed) };
                            (out, real_addr)
                        }))
                }).flatten();

                Ok(Box::new(future) as Box<_>)
            }
        }
    }

    #[inline]
    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

impl<Trans, PStore, PStoreRef> MuxedTransport for IdentifyTransport<Trans, PStoreRef>
where
    Trans: MuxedTransport + Clone + 'static,
    Trans::Output: AsyncRead + AsyncWrite,
    PStoreRef: Deref<Target = PStore> + Clone + 'static,
    for<'r> &'r PStore: Peerstore,
{
    type Incoming = Box<Future<Item = Self::IncomingUpgrade, Error = IoError>>;
    type IncomingUpgrade = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        let identify_upgrade = self.transport.clone().with_upgrade(IdentifyProtocolConfig);
        let peerstore = self.peerstore;
        let addr_ttl = self.addr_ttl;

        let future = self.transport.next_incoming().map(move |incoming| {
            let peerstore = peerstore.clone();
            let future = incoming.and_then(move |(connec, client_addr)| {
                for peer_id in peerstore.peers() {
                    let peer = match peerstore.peer(&peer_id) {
                        Some(p) => p,
                        None => continue,
                    };

                    if peer.addrs().any(|addr| addr == client_addr) {
                        debug!(target: "libp2p-identify", "Incoming substream from {} \
                                                               identified as {:?}", client_addr,
                                                               peer_id);
                        let out = IdentifyTransportOutput { socket: connec, observed_addr: None };
                        let ret = (out, AddrComponent::P2P(peer_id.into_bytes()).into());
                        return future::Either::A(future::ok(ret));
                    }
                }

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
                    .and_then(move |(identify, connec)| {
                        // Add the info to the peerstore and compute the "real" address of the
                        // node (in the form `/p2p/...`).
                        let (real_addr, observed);
                        match identify {
                            (IdentifyOutput::RemoteInfo { info, observed_addr }, old_addr) => {
                                observed = observed_addr;
                                real_addr = process_identify_info(&info, &*peerstore, old_addr, addr_ttl)?;
                            }
                            _ => unreachable!(
                                "the identify protocol guarantees that we receive remote \
                                 information when we dial a node"
                            ),
                        };

                        let out = IdentifyTransportOutput { socket: connec, observed_addr: Some(observed) };
                        Ok((out, real_addr))
                    });
                future::Either::B(future)
            });

            Box::new(future) as Box<Future<Item = _, Error = _>>
        });

        Box::new(future) as Box<_>
    }
}

/// Output of the identify transport.
#[derive(Debug, Clone)]
pub struct IdentifyTransportOutput<S> {
    /// The socket to communicate with the remote.
    pub socket: S,
    /// Address that the remote observes us as, if known.
    pub observed_addr: Option<Multiaddr>,
}

// If the multiaddress is in the form `/p2p/...`, turn it into a `PeerId`.
// Otherwise, return it as-is.
fn multiaddr_to_peerid(addr: Multiaddr) -> Result<PeerId, Multiaddr> {
    let components = addr.iter().collect::<Vec<_>>();
    if components.len() < 1 {
        return Err(addr);
    }

    match components.last() {
        Some(&AddrComponent::P2P(ref peer_id)) | Some(&AddrComponent::IPFS(ref peer_id)) => {
            // TODO: `peer_id` is sometimes in fact a CID here
            match PeerId::from_bytes(peer_id.clone()) {
                Ok(peer_id) => Ok(peer_id),
                Err(_) => Err(addr),
            }
        }
        _ => Err(addr),
    }
}

// When passed the information sent by a remote, inserts the remote into the given peerstore and
// returns a multiaddr of the format `/p2p/...` corresponding to this node.
//
// > **Note**: This function is highly-specific, but this precise behaviour is needed in multiple
// >           different places in the code.
fn process_identify_info<P>(
    info: &IdentifyInfo,
    peerstore: P,
    client_addr: Multiaddr,
    ttl: Duration,
) -> Result<Multiaddr, IoError>
where
    P: Peerstore,
{
    let peer_id = PeerId::from_public_key(&info.public_key);
    peerstore
        .peer_or_create(&peer_id)
        .add_addr(client_addr, ttl);
    Ok(AddrComponent::P2P(peer_id.into_bytes()).into())
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    extern crate tokio_core;

    use self::libp2p_tcp_transport::TcpConfig;
    use self::tokio_core::reactor::Core;
    use IdentifyTransport;
    use futures::{Future, Stream};
    use libp2p_peerstore::memory_peerstore::MemoryPeerstore;
    use libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
    use libp2p_core::Transport;
    use multiaddr::{AddrComponent, Multiaddr};
    use std::io::Error as IoError;
    use std::iter;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn dial_peer_id() {
        // When we dial an `/p2p/...` address, the `IdentifyTransport` should look into the
        // peerstore and dial one of the known multiaddresses of the node instead.

        #[derive(Debug, Clone)]
        struct UnderlyingTrans {
            inner: TcpConfig,
        }
        impl Transport for UnderlyingTrans {
            type Output = <TcpConfig as Transport>::Output;
            type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
            type ListenerUpgrade = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;
            type Dial = <TcpConfig as Transport>::Dial;
            #[inline]
            fn listen_on(
                self,
                _: Multiaddr,
            ) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
                unreachable!()
            }
            #[inline]
            fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
                assert_eq!(
                    addr,
                    "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap()
                );
                Ok(self.inner.dial(addr).unwrap_or_else(|_| panic!()))
            }
            #[inline]
            fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
                self.inner.nat_traversal(a, b)
            }
        }

        let peer_id = PeerId::from_public_key(&vec![1, 2, 3, 4]);

        let peerstore = MemoryPeerstore::empty();
        peerstore.peer_or_create(&peer_id).add_addr(
            "/ip4/127.0.0.1/tcp/12345".parse().unwrap(),
            Duration::from_secs(3600),
        );

        let mut core = Core::new().unwrap();
        let underlying = UnderlyingTrans {
            inner: TcpConfig::new(core.handle()),
        };
        let transport = IdentifyTransport::new(underlying, Arc::new(peerstore));

        let future = transport
            .dial(iter::once(AddrComponent::P2P(peer_id.into_bytes())).collect())
            .unwrap_or_else(|_| panic!())
            .then::<_, Result<(), ()>>(|_| Ok(()));

        let _ = core.run(future).unwrap();
    }
}
