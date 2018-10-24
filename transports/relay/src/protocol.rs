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
use copy;
use core::{ConnectionUpgrade, Endpoint, PeerId, Transport};
use futures::{stream, future::{self, Either::{A, B}, FutureResult}, prelude::*};
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Status, CircuitRelay_Type};
use std::{io, iter};
use tokio_io::{AsyncRead, AsyncWrite};
use utility::{io_err, is_success, status, Io, Peer};

/// Configuration for a connection upgrade that handles the relay protocol.
#[derive(Debug, Clone)]
pub struct RelayConfig<T> {
    /// Peer id of the local node.
    my_id: PeerId,
    /// When asked to relay a connection, the transport to use to reach the requested node.
    transport: T,
    /// If `allow_relays` is false this node can only be used as a destination but will not allow
    /// relaying streams to other destinations.
    allow_relays: bool
}

/// The `RelayConfig` upgrade can serve as destination or relay. Each mode needs a different
/// output type. As destination we want the stream to continue to be usable, whereas as relay
/// we pipe data from source to destination and do not want to use the stream in any other way.
/// Therefore, in the latter case we simply return a future that can be driven to completion
/// but otherwise the stream is not programmatically accessible.
pub enum RelayOutput<TStream> {
    /// The source is a relay `R` that relays the communication from someone `S` to us. Keep in
    /// mind that the multiaddress you have about this communication is the address of `R`.
    Stream {
        /// Stream of data to the original source.
        stream: TStream,
        /// Identifier of the original source `S`.
        src_peer_id: PeerId,
        // TODO: also provide the addresses ; however the semantics of these addresses is uncertain
    },

    /// We have been asked to relay communications to another node. Polling the future until it's
    /// ready will process the proxying.
    // TODO: provide more info for informative purposes for the user
    Sealed(Box<Future<Item=(), Error=io::Error> + Send>),
}

impl<C, T> ConnectionUpgrade<C> for RelayConfig<T>
where
    C: AsyncRead + AsyncWrite + Send + 'static,
    T: Transport + Clone + Send + 'static,
    T::Dial: Send,
    T::Listener: Send,
    T::ListenerUpgrade: Send,
    T::Output: AsyncRead + AsyncWrite + Send,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }

    type Output = RelayOutput<C>;
    type Future = Box<Future<Item=Self::Output, Error=io::Error> + Send>;

    fn upgrade(self, conn: C, _: (), _: Endpoint) -> Self::Future {
        let future = Io::new(conn).recv().and_then(move |(message, io)| {
            let msg = if let Some(m) = message {
                m
            } else {
                return A(A(future::err(io_err("no message received"))))
            };
            match msg.get_field_type() {
                CircuitRelay_Type::HOP if self.allow_relays => { // act as relay
                    B(A(self.on_hop(msg, io).map(|fut| RelayOutput::Sealed(Box::new(fut)))))
                }
                CircuitRelay_Type::STOP => { // act as destination
                    B(B(self.on_stop(msg, io)))
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

impl<T> RelayConfig<T>
where
    T: Transport + Clone + 'static,
    T::Dial: Send,      // TODO: remove
    T::Listener: Send,      // TODO: remove
    T::ListenerUpgrade: Send,      // TODO: remove
    T::Output: Send + AsyncRead + AsyncWrite,
{
    /// Builds a new `RelayConfig` with default options.
    pub fn new(my_id: PeerId, transport: T) -> RelayConfig<T> {
        RelayConfig { my_id, transport, allow_relays: true }
    }

    /// Sets whether we will allow requests for relaying connections.
    pub fn allow_relays(&mut self, val: bool) {
        self.allow_relays = val
    }

    /// HOP message handling (request to act as a relay).
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

        let dest = if let Some(peer) = Peer::from_message(msg.take_dstPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::HOP_DST_MULTIADDR_INVALID);
            return B(A(io.send(msg).and_then(|_| Err(io_err("invalid dest address")))))
        };

        let stop = stop_message(&from, &dest);

        let transport = self.transport.with_upgrade(TrivialUpgrade);
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
                if let Some(c) = ok {
                    // send STOP message to destination and expect back a SUCCESS message
                    let future = Io::new(c).send(stop)
                        .and_then(Io::recv)
                        .and_then(|(response, io)| {
                            let rsp = match response {
                                Some(m) => m,
                                None => return Err(io_err("no message from destination"))
                            };
                            if is_success(&rsp) {
                                Ok(io.into())
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
                    Ok(c) => {
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
                    let a = copy::flushing_copy(src_r, dst_w).map(|_| ());
                    let b = copy::flushing_copy(dst_r, src_w).map(|_| ());
                    a.select(b).map(|_| ()).map_err(|(e, _)| e)
                };
                Ok(future)
            });

        B(B(future))
    }

    /// STOP message handling (we are a destination)
    fn on_stop<C>(self, mut msg: CircuitRelay, io: Io<C>) -> impl Future<Item = RelayOutput<C>, Error = io::Error>
    where
        C: AsyncRead + AsyncWrite + 'static,
    {
        let from = if let Some(peer) = Peer::from_message(msg.take_srcPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::HOP_SRC_MULTIADDR_INVALID);
            return A(A(io.send(msg).and_then(|_| Err(io_err("invalid src address")))))
        };

        let dest = if let Some(peer) = Peer::from_message(msg.take_dstPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::STOP_DST_MULTIADDR_INVALID);
            return A(B(io.send(msg).and_then(|_| Err(io_err("invalid dest address")))))
        };

        if dest.id != self.my_id {
            let msg = status(CircuitRelay_Status::STOP_RELAY_REFUSED);
            return B(A(io.send(msg).and_then(|_| Err(io_err("destination id mismatch")))))
        }

        B(B(io.send(status(CircuitRelay_Status::SUCCESS)).map(move |stream| {
            RelayOutput::Stream {
                stream: stream.into(),
                src_peer_id: from.id,
            }
        })))
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

/// Dummy connection upgrade that negotiates the relay protocol and returns the socket.
#[derive(Debug, Clone)]
struct TrivialUpgrade;

impl<C> ConnectionUpgrade<C> for TrivialUpgrade
where
    C: AsyncRead + AsyncWrite + 'static
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }

    type Output = C;
    type Future = FutureResult<Self::Output, io::Error>;

    fn upgrade(self, conn: C, _: (), _: Endpoint) -> Self::Future {
        future::ok(conn)
    }
}

/// Connection upgrade that negotiates the relay protocol, then negotiates with the target for it
/// to act as destination.
#[derive(Debug, Clone)]
pub(crate) struct Source(pub(crate) CircuitRelay);

impl<C> ConnectionUpgrade<C> for Source
where
    C: AsyncRead + AsyncWrite + Send + 'static,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }

    type Output = C;
    type Future = Box<Future<Item=Self::Output, Error=io::Error> + Send>;

    fn upgrade(self, conn: C, _: (), _: Endpoint) -> Self::Future {
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

