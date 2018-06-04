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
use libp2p_core::{MuxedTransport, Transport, PeerId};
use multiaddr::{AddrComponent, Multiaddr};
use protocol::{IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use tokio_io::{AsyncRead, AsyncWrite};

/// Interface for storing and retrieving information about peers.
pub trait IdentifyPeerInterface {
    /// Iterator for the list of multiaddresses known for a peer.
    type Iter: Iterator<Item = Multiaddr>;

    /// Called when the identify transport successfully identified a remote.
    ///
    /// The implementation is expected to turn the public key into a peer id, and associate the
    /// given multiaddress to it.
    fn store_info(&self, info: &IdentifyInfo, multiaddr: Multiaddr);

    /// Returns the peer ID of a multiaddress, if any.
    fn peer_id_by_multiaddress(&self, multiaddr: &Multiaddr) -> Option<PeerId>;

    /// Retrieves the list of multiaddresses known for a peer. Can be empty if none is known.
    fn multiaddresses_by_peer_id(&self, peer_id: PeerId) -> Self::Iter;
}

/// Implementation of `Transport`. See [the crate root description](index.html).
#[derive(Debug, Clone)]
pub struct IdentifyTransport<Trans, PInterface> {
    transport: Trans,
    peer_interface: PInterface,
}

impl<Trans, PInterface> IdentifyTransport<Trans, PInterface> {
    /// Creates an `IdentifyTransport` that wraps around the given transport and peer_interface.
    #[inline]
    pub fn new(transport: Trans, peer_interface: PInterface) -> Self {
        IdentifyTransport {
            transport: transport,
            peer_interface: peer_interface,
        }
    }
}

impl<Trans, PInterface> Transport for IdentifyTransport<Trans, PInterface>
where
    Trans: Transport + Clone + 'static, // TODO: 'static :(
    Trans::Output: AsyncRead + AsyncWrite,
    PInterface: IdentifyPeerInterface + Clone + 'static,        // TODO: 'static :-/
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
                    peer_interface: self.peer_interface,
                };
                return Err((id, addr));
            }
        };

        let identify_upgrade = self.transport.with_upgrade(IdentifyProtocolConfig);
        let peer_interface = self.peer_interface;

        let listener = listener.map(move |connec| {
            let peer_interface = peer_interface.clone();
            let identify_upgrade = identify_upgrade.clone();
            let fut = connec.and_then(move |(connec, client_addr)| {
                if let Some(peer_id) = peer_interface.peer_id_by_multiaddress(&client_addr) {
                    debug!("Incoming substream from {} identified as {:?}", client_addr, peer_id);
                    let out = IdentifyTransportOutput { socket: connec, observed_addr: None };
                    let ret = (out, AddrComponent::P2P(peer_id.into_bytes()).into());
                    return future::Either::A(future::ok(ret));
                }

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
                    .and_then(move |((identify, original_addr), connec)| {
                        // Compute the "real" address of the node (in the form `/p2p/...`) and
                        // add it to the peer_interface.
                        let (observed, real_addr);
                        match identify {
                            IdentifyOutput::RemoteInfo { info, observed_addr } => {
                                observed = observed_addr;
                                real_addr = process_identify_info(
                                    &info,
                                    &peer_interface,
                                    original_addr.clone(),
                                )?;
                            },
                            _ => unreachable!(
                                "the identify protocol guarantees that we receive \
                                 remote information when we dial a node"
                            ),
                        };

                        debug!("Identified {} as {}", original_addr, real_addr);
                        let out = IdentifyTransportOutput { socket: connec, observed_addr: Some(observed) };
                        Ok((out, real_addr))
                    })
                    .map_err(move |err| {
                        debug!("Failed to identify incoming {}", client_addr);
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
                // peer_interface) one by one.
                let addrs = self.peer_interface.multiaddresses_by_peer_id(peer_id.clone())
                    .collect::<Vec<_>>()        // TODO: don't collect?
                    .into_iter();

                trace!("Try dialing peer ID {:?} ; {} multiaddrs loaded from peer_interface", peer_id, addrs.len());

                let transport = self.transport;
                let future = stream::iter_ok(addrs)
                    // Try to dial each address through the transport.
                    .filter_map(move |addr| {
                        match transport.clone().dial(addr) {
                            Ok(dial) => Some(dial),
                            Err((_, addr)) => {
                                debug!("Address {} not supported by underlying transport", addr);
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
                                debug!("Successfully dialed peer {:?} through {}", peer_id, inner_addr);
                                let out = IdentifyTransportOutput { socket: connec, observed_addr: None };
                                Ok((out, inner_addr))
                            },
                            None => {
                                debug!("All multiaddresses failed when dialing peer {:?}", peer_id);
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

                trace!("Pass through when dialing {}", addr);

                // We dial a first time the node and upgrade it to identify.
                let dial = match identify_upgrade.dial(addr) {
                    Ok(d) => d,
                    Err((_, addr)) => {
                        let id = IdentifyTransport {
                            transport,
                            peer_interface: self.peer_interface,
                        };
                        return Err((id, addr));
                    }
                };

                let peer_interface = self.peer_interface;

                let future = dial.and_then(move |identify| {
                    // On success, store the information in the peer_interface and compute the
                    // "real" address of the node (of the form `/p2p/...`).
                    let (real_addr, old_addr, observed);
                    match identify {
                        (IdentifyOutput::RemoteInfo { info, observed_addr }, a) => {
                            old_addr = a.clone();
                            observed = observed_addr;
                            real_addr = process_identify_info(&info, &peer_interface, a)?;
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

impl<Trans, PInterface> MuxedTransport for IdentifyTransport<Trans, PInterface>
where
    Trans: MuxedTransport + Clone + 'static,        // TODO: 'static :-/
    Trans::Output: AsyncRead + AsyncWrite,
    PInterface: IdentifyPeerInterface + Clone + 'static,        // TODO: 'static :-/
{
    type Incoming = Box<Future<Item = Self::IncomingUpgrade, Error = IoError>>;
    type IncomingUpgrade = Box<Future<Item = (Self::Output, Multiaddr), Error = IoError>>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        let identify_upgrade = self.transport.clone().with_upgrade(IdentifyProtocolConfig);
        let peer_interface = self.peer_interface;

        let future = self.transport.next_incoming().map(move |incoming| {
            let peer_interface = peer_interface.clone();
            let future = incoming.and_then(move |(connec, client_addr)| {
                if let Some(peer_id) = peer_interface.peer_id_by_multiaddress(&client_addr) {
                    debug!("Incoming substream from {} identified as {:?}", client_addr, peer_id);
                    let out = IdentifyTransportOutput { socket: connec, observed_addr: None };
                    let ret = (out, AddrComponent::P2P(peer_id.into_bytes()).into());
                    return future::Either::A(future::ok(ret));
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
                        // Add the info to the peer_interface and compute the "real" address of the
                        // node (in the form `/p2p/...`).
                        let (real_addr, observed);
                        match identify {
                            (IdentifyOutput::RemoteInfo { info, observed_addr }, old_addr) => {
                                observed = observed_addr;
                                real_addr = process_identify_info(&info, &peer_interface, old_addr)?;
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

// When passed the information sent by a remote, inserts the remote into the given peer_interface and
// returns a multiaddr of the format `/p2p/...` corresponding to this node.
//
// > **Note**: This function is highly-specific, but this precise behaviour is needed in multiple
// >           different places in the code.
fn process_identify_info<P>(
    info: &IdentifyInfo,
    peer_interface: &P,
    client_addr: Multiaddr,
) -> Result<Multiaddr, IoError>
where
    P: ?Sized + IdentifyPeerInterface,
{
    let peer_id = PeerId::from_public_key(&info.public_key);
    peer_interface.store_info(info, client_addr);
    Ok(AddrComponent::P2P(peer_id.into_bytes()).into())
}

#[cfg(test)]
mod tests {
    extern crate libp2p_peerstore;
    extern crate libp2p_tcp_transport;
    extern crate tokio_core;

    use self::libp2p_tcp_transport::TcpConfig;
    use self::tokio_core::reactor::Core;
    use {IdentifyTransport, IdentifyInfo, IdentifyPeerInterface};
    use futures::{Future, Stream};
    use self::libp2p_peerstore::memory_peerstore::MemoryPeerstore;
    use self::libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
    use libp2p_core::Transport;
    use multiaddr::{AddrComponent, Multiaddr};
    use std::io::Error as IoError;
    use std::iter;
    use std::sync::Arc;
    use std::time::Duration;
    use std::vec::IntoIter as VecIntoIter;

    #[derive(Clone)]
    struct PeerstoreWrapper(Arc<MemoryPeerstore>);
    impl IdentifyPeerInterface for PeerstoreWrapper {
        type Iter = VecIntoIter<Multiaddr>;

        fn store_info(&self, _info: &IdentifyInfo, _multiaddr: Multiaddr) {
            unimplemented!()
        }

        fn peer_id_by_multiaddress(&self, _multiaddr: &Multiaddr) -> Option<PeerId> {
            unimplemented!()
        }

        fn multiaddresses_by_peer_id(&self, peer_id: PeerId) -> Self::Iter {
            match self.0.peer(&peer_id) {
                Some(peer) => peer.addrs().collect::<Vec<_>>().into_iter(),
                None => Vec::new().into_iter(),
            }
        }
    }

    #[test]
    fn dial_peer_id() {
        // When we dial an `/p2p/...` address, the `IdentifyTransport` should look into the
        // peer_interface and dial one of the known multiaddresses of the node instead.

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

        let peer_interface = MemoryPeerstore::empty();
        peer_interface.peer_or_create(&peer_id).add_addr(
            "/ip4/127.0.0.1/tcp/12345".parse().unwrap(),
            Duration::from_secs(3600),
        );

        let mut core = Core::new().unwrap();
        let underlying = UnderlyingTrans {
            inner: TcpConfig::new(core.handle()),
        };
        let transport = IdentifyTransport::new(underlying, PeerstoreWrapper(Arc::new(peer_interface)));

        let future = transport
            .dial(iter::once(AddrComponent::P2P(peer_id.into_bytes())).collect())
            .unwrap_or_else(|_| panic!())
            .then::<_, Result<(), ()>>(|_| Ok(()));

        let _ = core.run(future).unwrap();
    }
}
