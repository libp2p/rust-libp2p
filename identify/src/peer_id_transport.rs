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

use futures::{future, stream, Future, Stream};
use identify_transport::{IdentifyTransport, IdentifyTransportOutcome};
use libp2p_core::{PeerId, MuxedTransport, Transport};
use multiaddr::{AddrComponent, Multiaddr};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `Transport`. See [the crate root description](index.html).
#[derive(Clone)]
pub struct PeerIdTransport<Trans, AddrRes> {
    transport: IdentifyTransport<Trans>,
    addr_resolver: AddrRes,
}

impl<Trans, AddrRes> PeerIdTransport<Trans, AddrRes> {
    /// Creates an `PeerIdTransport` that wraps around the given transport and address resolver.
    #[inline]
    pub fn new(transport: Trans, addr_resolver: AddrRes) -> Self {
        PeerIdTransport {
            transport: IdentifyTransport::new(transport),
            addr_resolver,
        }
    }
}

impl<Trans, AddrRes, AddrResOut> Transport for PeerIdTransport<Trans, AddrRes>
where
    Trans: Transport + Clone + 'static, // TODO: 'static :(
    Trans::Output: AsyncRead + AsyncWrite,
    AddrRes: Fn(PeerId) -> AddrResOut + 'static,       // TODO: 'static :(
    AddrResOut: IntoIterator<Item = Multiaddr> + 'static,       // TODO: 'static :(
{
    type Output = PeerIdTransportOutput<Trans::Output>;
    type MultiaddrFuture = Box<Future<Item = Multiaddr, Error = IoError>>;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>>;
    type Dial = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        // Note that `listen_on` expects a "regular" multiaddr (eg. `/ip/.../tcp/...`),
        // and not `/p2p/<foo>`.

        let (listener, listened_addr) = match self.transport.listen_on(addr) {
            Ok((listener, addr)) => (listener, addr),
            Err((inner, addr)) => {
                let id = PeerIdTransport {
                    transport: inner,
                    addr_resolver: self.addr_resolver,
                };

                return Err((id, addr));
            }
        };

        let listener = listener.map(move |connec| {
            let fut = connec
                .and_then(move |(connec, client_addr)| {
                    client_addr.map(move |addr| (connec, addr))
                })
                .map(move |(connec, original_addr)| {
                    debug!("Successfully incoming connection from {}", original_addr);
                    let info = connec.info.shared();
                    let out = PeerIdTransportOutput {
                        socket: connec.socket,
                        info: Box::new(info.clone()
                            .map(move |info| (*info).clone())
                            .map_err(move |err| { let k = err.kind(); IoError::new(k, err) })),
                        original_addr: original_addr.clone(),
                    };
                    let real_addr = Box::new(info
                        .map_err(move |err| { let k = err.kind(); IoError::new(k, err) })
                        .map(move |info| {
                            let peer_id = info.info.public_key.clone().into_peer_id();
                            debug!("Identified {} as {:?}", original_addr, peer_id);
                            AddrComponent::P2P(peer_id.into_bytes()).into()
                        })) as Box<Future<Item = _, Error = _>>;
                    (out, real_addr)
                });

            Box::new(fut) as Box<Future<Item = _, Error = _>>
        });

        Ok((Box::new(listener) as Box<_>, listened_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match multiaddr_to_peerid(addr.clone()) {
            Ok(peer_id) => {
                // If the multiaddress is a peer ID, try each known multiaddress (taken from the
                // address resolved) one by one.
                let addrs = {
                    let resolver = &self.addr_resolver;
                    resolver(peer_id.clone()).into_iter()
                };

                trace!("Try dialing peer ID {:?} ; loading multiaddrs from addr resolver", peer_id);

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
                    .and_then(move |(connec, _)| {
                        match connec {
                            Some(connec) => Ok((connec, peer_id)),
                            None => {
                                debug!("All multiaddresses failed when dialing peer {:?}", peer_id);
                                Err(IoError::new(IoErrorKind::Other, "couldn't find any multiaddress for peer"))
                            },
                        }
                    })
                    .and_then(move |((connec, original_addr), peer_id)| {
                        original_addr.map(move |addr| (connec, addr, peer_id))
                    })
                    .and_then(move |(connec, original_addr, peer_id)| {
                        debug!("Successfully dialed peer {:?} through {}", peer_id, original_addr);
                        let out = PeerIdTransportOutput {
                            socket: connec.socket,
                            info: connec.info,
                            original_addr: original_addr,
                        };
                        // Replace the multiaddress with the one of the form `/p2p/...` or `/ipfs/...`.
                        Ok((out, Box::new(future::ok(addr)) as Box<Future<Item = _, Error = _>>))
                    });

                Ok(Box::new(future) as Box<_>)
            }

            Err(addr) => {
                // If the multiaddress is something else, propagate it to the underlying transport.
                trace!("Propagating {} to the underlying transport", addr);
                let dial = match self.transport.dial(addr) {
                    Ok(d) => d,
                    Err((inner, addr)) => {
                        let id = PeerIdTransport {
                            transport: inner,
                            addr_resolver: self.addr_resolver,
                        };
                        return Err((id, addr));
                    }
                };

                let future = dial
                    .and_then(move |(connec, original_addr)| {
                        original_addr.map(move |addr| (connec, addr))
                    })
                    .map(move |(connec, original_addr)| {
                        debug!("Successfully dialed {}", original_addr);
                        let info = connec.info.shared();
                        let out = PeerIdTransportOutput {
                            socket: connec.socket,
                            info: Box::new(info.clone()
                                .map(move |info| (*info).clone())
                                .map_err(move |err| { let k = err.kind(); IoError::new(k, err) })),
                            original_addr: original_addr.clone(),
                        };
                        let real_addr = Box::new(info
                            .map_err(move |err| { let k = err.kind(); IoError::new(k, err) })
                            .map(move |info| {
                                let peer_id = info.info.public_key.clone().into_peer_id();
                                debug!("Identified {} as {:?}", original_addr, peer_id);
                                AddrComponent::P2P(peer_id.into_bytes()).into()
                            })) as Box<Future<Item = _, Error = _>>;
                        (out, real_addr)
                    });

                Ok(Box::new(future) as Box<_>)
            }
        }
    }

    #[inline]
    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

impl<Trans, AddrRes, AddrResOut> MuxedTransport for PeerIdTransport<Trans, AddrRes>
where
    Trans: MuxedTransport + Clone + 'static,
    Trans::Output: AsyncRead + AsyncWrite,
    AddrRes: Fn(PeerId) -> AddrResOut + 'static,       // TODO: 'static :(
    AddrResOut: IntoIterator<Item = Multiaddr> + 'static,   // TODO: 'static :(
{
    type Incoming = Box<Future<Item = Self::IncomingUpgrade, Error = IoError>>;
    type IncomingUpgrade = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        let future = self.transport.next_incoming().map(move |incoming| {
            let future = incoming
                .and_then(move |(connec, original_addr)| {
                    original_addr.map(move |addr| (connec, addr))
                })
                .map(move |(connec, original_addr)| {
                    debug!("Successful incoming substream from {}", original_addr);
                    let info = connec.info.shared();
                    let out = PeerIdTransportOutput {
                        socket: connec.socket,
                        info: Box::new(info.clone()
                            .map(move |info| (*info).clone())
                            .map_err(move |err| { let k = err.kind(); IoError::new(k, err) })),
                        original_addr: original_addr.clone(),
                    };
                    let real_addr = Box::new(info
                        .map_err(move |err| { let k = err.kind(); IoError::new(k, err) })
                        .map(move |info| {
                            let peer_id = info.info.public_key.clone().into_peer_id();
                            debug!("Identified {} as {:?}", original_addr, peer_id);
                            AddrComponent::P2P(peer_id.into_bytes()).into()
                        })) as Box<Future<Item = _, Error = _>>;
                    (out, real_addr)
                });

            Box::new(future) as Box<Future<Item = _, Error = _>>
        });

        Box::new(future) as Box<_>
    }
}

/// Output of the identify transport.
pub struct PeerIdTransportOutput<S> {
    /// The socket to communicate with the remote.
    pub socket: S,

    /// Identification of the remote.
    /// This may not be known immediately, hence why we use a future.
    pub info: Box<Future<Item = IdentifyTransportOutcome, Error = IoError>>,

    /// Original address of the remote.
    /// This layer turns the address of the remote into the `/p2p/...` form, but stores the
    /// original address in this field.
    pub original_addr: Multiaddr,
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

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    extern crate tokio_current_thread;

    use self::libp2p_tcp_transport::TcpConfig;
    use PeerIdTransport;
    use futures::{Future, Stream};
    use libp2p_core::{Transport, PeerId, PublicKey};
    use multiaddr::{AddrComponent, Multiaddr};
    use std::io::Error as IoError;
    use std::iter;

    #[test]
    fn dial_peer_id() {
        // When we dial an `/p2p/...` address, the `PeerIdTransport` should look into the
        // peerstore and dial one of the known multiaddresses of the node instead.

        #[derive(Debug, Clone)]
        struct UnderlyingTrans {
            inner: TcpConfig,
        }
        impl Transport for UnderlyingTrans {
            type Output = <TcpConfig as Transport>::Output;
            type MultiaddrFuture = <TcpConfig as Transport>::MultiaddrFuture;
            type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
            type ListenerUpgrade = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>>;
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

        let peer_id = PeerId::from_public_key(PublicKey::Ed25519(vec![1, 2, 3, 4]));

        let underlying = UnderlyingTrans {
            inner: TcpConfig::new(),
        };
        let transport = PeerIdTransport::new(underlying, {
            let peer_id = peer_id.clone();
            move |addr| {
                assert_eq!(addr, peer_id);
                vec!["/ip4/127.0.0.1/tcp/12345".parse().unwrap()]
            }
        });

        let future = transport
            .dial(iter::once(AddrComponent::P2P(peer_id.into_bytes())).collect())
            .unwrap_or_else(|_| panic!())
            .then::<_, Result<(), ()>>(|_| Ok(()));

        let _ = tokio_current_thread::block_on_all(future).unwrap();
    }
}
