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

use bytes::Bytes;
use core::{ConnectionUpgrade, Endpoint, Transport};
use core::upgrade::SimpleProtocol;
use futures::{stream, future::{self, Either::{A, B}}, prelude::*};
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Status, CircuitRelay_Type};
use multiaddr::Multiaddr;
use peerstore::{PeerAccess, PeerId, Peerstore};
use std::{io, iter, ops::Deref};
use tokio_io::{io as aio, AsyncRead, AsyncWrite};
use utility::{io_err, is_success, status, Io, Peer};

#[derive(Debug, Clone)]
pub struct RelayConfig<T, P> {
    my_id: PeerId,
    transport: T,
    peers: P,
    // If `allow_relays` is false this node can only be used as a
    // destination but will not allow relaying streams to other
    // destinations.
    allow_relays: bool
}

// The `RelayConfig` upgrade can serve as destination or relay. Each mode needs a different
// output type. As destination we want the stream to continue to be usable, whereas as relay
// we pipe data from source to destination and do not want to use the stream in any other way.
// Therefore, in the latter case we simply return a future that can be driven to completion
// but otherwise the stream is not programmatically accessible.
pub enum Output<C> {
    Stream(C),
    Sealed(Box<Future<Item=(), Error=io::Error>>)
}

impl<C, T, P, S> ConnectionUpgrade<C> for RelayConfig<T, P>
where
    C: AsyncRead + AsyncWrite + 'static,
    T: Transport + Clone + 'static,
    T::Output: AsyncRead + AsyncWrite,
    P: Deref<Target=S> + Clone + 'static,
    S: 'static,
    for<'a> &'a S: Peerstore
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }

    type Output = Output<C>;
    type Future = Box<Future<Item=Self::Output, Error=io::Error>>;

    fn upgrade(self, conn: C, _: (), _: Endpoint, _: &Multiaddr) -> Self::Future {
        let future = Io::new(conn).recv().and_then(move |(message, io)| {
            let msg = if let Some(m) = message {
                m
            } else {
                return A(A(future::err(io_err("no message received"))))
            };
            match msg.get_field_type() {
                CircuitRelay_Type::HOP if self.allow_relays => { // act as relay
                    B(A(self.on_hop(msg, io).map(|fut| Output::Sealed(Box::new(fut)))))
                }
                CircuitRelay_Type::STOP => { // act as destination
                    B(B(self.on_stop(msg, io).map(Output::Stream)))
                }
                other => {
                    debug!("invalid message type: {:?}", other);
                    let resp = status(CircuitRelay_Status::MALFORMED_MESSAGE);
                    A(B(io.send(resp).and_then(|_| Err(io_err("invalid message type")))))
                }
            }
        });
        Box::new(future)
    }
}

impl<T, P, S> RelayConfig<T, P>
where
    T: Transport + Clone + 'static,
    T::Output: AsyncRead + AsyncWrite,
    P: Deref<Target = S> + Clone + 'static,
    for<'a> &'a S: Peerstore,
{
    pub fn new(my_id: PeerId, transport: T, peers: P) -> RelayConfig<T, P> {
        RelayConfig { my_id, transport, peers, allow_relays: true }
    }

    pub fn allow_relays(&mut self, val: bool) {
        self.allow_relays = val
    }

    // HOP message handling (relay mode).
    fn on_hop<C>(self, mut msg: CircuitRelay, io: Io<C>) -> impl Future<Item=impl Future<Item=(), Error=io::Error>, Error=io::Error>
    where
        C: AsyncRead + AsyncWrite + 'static,
    {
        let from = if let Some(peer) = Peer::from_message(msg.take_srcPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::HOP_SRC_MULTIADDR_INVALID);
            return A(io.send(msg).and_then(|_| Err(io_err("invalid src address"))))
        };

        let mut dest = if let Some(peer) = Peer::from_message(msg.take_dstPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::HOP_DST_MULTIADDR_INVALID);
            return B(A(io.send(msg).and_then(|_| Err(io_err("invalid dest address")))))
        };

        if dest.addrs.is_empty() {
            // Add locally know addresses of destination
            if let Some(peer) = self.peers.peer(&dest.id) {
                dest.addrs.extend(peer.addrs())
            }
        }

        let stop = stop_message(&from, &dest);

        let upgrade = SimpleProtocol::new("/libp2p/relay/circuit/0.1.0", Ok);
        let transport = self.transport.with_upgrade(upgrade);
        let dest_id = dest.id;
        let future = stream::iter_ok(dest.addrs.into_iter())
            .and_then(move |dest_addr| {
                transport.clone().dial(dest_addr).map_err(|_| io_err("failed to dial"))
            })
            .and_then(|dial| dial)
            .then(|result| Ok(result.ok()))
            .filter_map(|result| result)
            .into_future()
            .map_err(|(err, _stream)| err)
            .and_then(move |(ok, _stream)| {
                if let Some((c, a)) = ok {
                    // send STOP message to destination and expect back a SUCCESS message
                    let future = Io::new(c).send(stop)
                        .and_then(Io::recv)
                        .and_then(|(response, io)| {
                            let rsp = match response {
                                Some(m) => m,
                                None => return Err(io_err("no message from destination"))
                            };
                            if is_success(&rsp) {
                                Ok((io.into(), a))
                            } else {
                                Err(io_err("no success response from relay"))
                            }
                        });
                    A(future)
                } else {
                    B(future::err(io_err(format!("could not dial to {:?}", dest_id))))
                }
            })
            // signal success or failure to source
            .then(move |result| {
                match result {
                    Ok((c, _)) => {
                        let msg = status(CircuitRelay_Status::SUCCESS);
                        A(io.send(msg).map(|io| (io.into(), c)))
                    }
                    Err(e) => {
                        let msg = status(CircuitRelay_Status::HOP_CANT_DIAL_DST);
                        B(io.send(msg).and_then(|_| Err(e)))
                    }
                }
            })
            // return future for bidirectional data transfer
            .and_then(move |(src, dst)| {
                let future = {
                    let (src_r, src_w) = src.split();
                    let (dst_r, dst_w) = dst.split();
                    let a = aio::copy(src_r, dst_w).map(|_| ());
                    let b = aio::copy(dst_r, src_w).map(|_| ());
                    a.select(b).map(|_| ()).map_err(|(e, _)| e)
                };
                Ok(future)
            });

        B(B(future))
    }

    // STOP message handling (destination mode)
    fn on_stop<C>(self, mut msg: CircuitRelay, io: Io<C>) -> impl Future<Item=C, Error=io::Error>
    where
        C: AsyncRead + AsyncWrite + 'static,
    {
        let dest = if let Some(peer) = Peer::from_message(msg.take_dstPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::STOP_DST_MULTIADDR_INVALID);
            return A(io.send(msg).and_then(|_| Err(io_err("invalid dest address"))))
        };

        if dest.id != self.my_id {
            let msg = status(CircuitRelay_Status::STOP_RELAY_REFUSED);
            return B(A(io.send(msg).and_then(|_| Err(io_err("destination id mismatch")))))
        }

        B(B(io.send(status(CircuitRelay_Status::SUCCESS)).map(Io::into)))
    }
}

fn stop_message(from: &Peer, dest: &Peer) -> CircuitRelay {
    let mut msg = CircuitRelay::new();
    msg.set_field_type(CircuitRelay_Type::STOP);

    let mut f = CircuitRelay_Peer::new();
    f.set_id(from.id.as_bytes().to_vec());
    for a in &from.addrs {
        f.mut_addrs().push(a.to_bytes())
    }
    msg.set_srcPeer(f);

    let mut d = CircuitRelay_Peer::new();
    d.set_id(dest.id.as_bytes().to_vec());
    for a in &dest.addrs {
        d.mut_addrs().push(a.to_bytes())
    }
    msg.set_dstPeer(d);

    msg
}

#[derive(Debug, Clone)]
pub(crate) struct Source(pub(crate) CircuitRelay);

impl<C> ConnectionUpgrade<C> for Source
where
    C: AsyncRead + AsyncWrite + 'static,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }

    type Output = C;
    type Future = Box<Future<Item=Self::Output, Error=io::Error>>;

    fn upgrade(self, conn: C, _: (), _: Endpoint, _: &Multiaddr) -> Self::Future {
        let future = Io::new(conn)
            .send(self.0)
            .and_then(Io::recv)
            .and_then(|(response, io)| {
                let rsp = match response {
                    Some(m) => m,
                    None => return Err(io_err("no message from relay")),
                };
                if is_success(&rsp) {
                    Ok(io.into())
                } else {
                    Err(io_err("no success response from relay"))
                }
            });
        Box::new(future)
    }
}

