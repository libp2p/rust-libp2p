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
use core::{upgrade, ConnectionUpgrade, Endpoint, Multiaddr, PeerId};
use futures::{future::{self, Either::{A, B}, FutureResult}, prelude::*};
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Status, CircuitRelay_Type};
use std::{io, iter};
use tokio_io::{AsyncRead, AsyncWrite};
use utility::{io_err, is_success, status, Io, Peer};

/// Configuration for a connection upgrade that handles the relay protocol.
#[derive(Debug, Clone)]
pub struct RelayConfig {
}

/// The `RelayConfig` upgrade can serve as destination or relay. Each mode needs a different
/// output type. As destination we want the stream to continue to be usable, whereas as relay
/// we pipe data from source to destination and do not want to use the stream in any other way.
/// Therefore, in the latter case we simply return a future that can be driven to completion
/// but otherwise the stream is not programmatically accessible.
// TODO: debug
pub enum RelayOutput<TStream> {
    /// We are successfully connected as a dialer, and we can now request the remote to act as a
    /// proxy.
    ProxyRequest(RelayProxyRequest<TStream>),
    /// We have been asked to become a destination.
    DestinationRequest(RelayDestinationRequest<TStream>),
    /// We have been asked to relay communications to another node.
    HopRequest(RelayHopRequest<TStream>),
}

/// We are successfully connected as a dialer, and we can now request the remote to act as a proxy.
// TODO: debug
#[must_use = "There is no point in opening a request if you don't use"]
pub struct RelayProxyRequest<TStream> {
    /// The stream of the destination.
    stream: Io<TStream>,
}

impl<TStream> RelayProxyRequest<TStream>
where TStream: AsyncRead + AsyncWrite + 'static
{
    /// Request proxying to a destination.
    pub fn request(self, dest_id: PeerId, dest_addresses: impl IntoIterator<Item = Multiaddr>)
        -> impl Future<Item = TStream, Error = io::Error>
    {
        let msg = hop_message(&Peer {
            id: dest_id,
            addrs: dest_addresses.into_iter().collect(),
        });

        self.stream
            .send(msg)
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
            })
    }
}

/// Request from a remote for us to become a destination.
// TODO: debug
#[must_use = "A destination request should be either accepted or denied"]
pub struct RelayDestinationRequest<TStream> {
    /// The stream to the source.
    stream: Io<TStream>,
    /// Source of the request.
    from: Peer,
}

impl<TStream> RelayDestinationRequest<TStream>
where TStream: AsyncRead + AsyncWrite + 'static
{
    /// Peer id of the source that is being relayed.
    pub fn source_id(&self) -> &PeerId {
        &self.from.id
    }

    // TODO: addresses

    /// Accepts the request.
    pub fn accept(self) -> impl Future<Item = TStream, Error = io::Error> {
        // send a SUCCESS message
        self.stream
            .send(status(CircuitRelay_Status::SUCCESS))
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
            })
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    #[inline]
    pub fn deny(self) -> impl Future<Item = (), Error = io::Error> {
        // TODO: correct status
        let message = deny_message(CircuitRelay_Status::STOP_RELAY_REFUSED);
        self.stream.send(message).map(|_| ())
    }
}

/// Request from a remote for us to relay communications to another node.
// TODO: debug
#[must_use = "A HOP request should be either accepted or denied"]
pub struct RelayHopRequest<TStream> {
    /// The stream to the source.
    stream: Io<TStream>,
    /// Target of the request.
    dest: Peer,
}

impl<TStream> RelayHopRequest<TStream>
where TStream: AsyncRead + AsyncWrite + 'static
{
    /// Accepts the request by providing a stream to the destination.
    ///
    /// The `dest_stream` should be a brand new dialing substream. This method will negotiate the
    /// `relay` protocol on it, send a relay message, and then relay to it the connection from the
    /// source.
    ///
    /// > **Note**: It is important that you process the `Future` that this method returns,
    /// >           otherwise the relaying will not work.
    pub fn fulfill<TDestStream>(self, dest_stream: TDestStream) -> impl Future<Item = (), Error = io::Error>
    where TDestStream: AsyncRead + AsyncWrite + 'static
    {
        let source_stream = self.stream;
        let stop = stop_message(&Peer::from_message(CircuitRelay_Peer::new()).unwrap(), &self.dest);
        upgrade::apply(dest_stream, TrivialUpgrade, Endpoint::Dialer)
            .and_then(|dest_stream| {
                // send STOP message to destination and expect back a SUCCESS message
                Io::new(dest_stream).send(stop)
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
                    })
            })
            // signal success or failure to source
            .then(move |result| {
                match result {
                    Ok(c) => {
                        let msg = status(CircuitRelay_Status::SUCCESS);
                        A(source_stream.send(msg).map(|io| (io.into(), c)))
                    }
                    Err(e) => {
                        let msg = status(CircuitRelay_Status::HOP_CANT_DIAL_DST);
                        B(source_stream.send(msg).and_then(|_| Err(e)))
                    }
                }
            })
            // return future for bidirectional data transfer
            .and_then(move |(src, dst)| {
                let (src_r, src_w) = src.split();
                let (dst_r, dst_w) = dst.split();
                let a = copy::flushing_copy(src_r, dst_w).map(|_| ());
                let b = copy::flushing_copy(dst_r, src_w).map(|_| ());
                a.select(b).map(|_| ()).map_err(|(e, _)| e)
            })
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    #[inline]
    pub fn deny(self) -> impl Future<Item = (), Error = io::Error> {
        // TODO: correct status
        let message = deny_message(CircuitRelay_Status::HOP_CANT_RELAY_TO_SELF);
        self.stream.send(message).map(|_| ())
    }
}

impl<C> ConnectionUpgrade<C> for RelayConfig
where
    C: AsyncRead + AsyncWrite + Send + 'static,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }

    type Output = RelayOutput<C>;
    type Future = Box<Future<Item=Self::Output, Error=io::Error> + Send>;

    fn upgrade(self, conn: C, _: (), endpoint: Endpoint) -> Self::Future {
        if let Endpoint::Dialer = endpoint {
            let future = future::ok(RelayOutput::ProxyRequest(RelayProxyRequest {
                stream: Io::new(conn),
            }));
            Box::new(future)
        } else {
            let future = Io::new(conn).recv().and_then(move |(message, io)| {
                let msg = if let Some(m) = message {
                    m
                } else {
                    return A(A(future::err(io_err("no message received"))))
                };
                match msg.get_field_type() {
                    CircuitRelay_Type::HOP => { // act as relay
                        B(A(self.on_hop(msg, io)))
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
}

impl RelayConfig {
    /// Builds a new `RelayConfig` with default options.
    pub fn new() -> RelayConfig {
        RelayConfig {}
    }

    /// HOP message handling (request to act as a relay).
    fn on_hop<C>(self, mut msg: CircuitRelay, io: Io<C>) -> impl Future<Item=RelayOutput<C>, Error=io::Error>
    where
        C: AsyncRead + AsyncWrite + 'static,
    {
        let from = if let Some(peer) = Peer::from_message(msg.take_srcPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::HOP_SRC_MULTIADDR_INVALID);
            return B(A(io.send(msg).and_then(|_| Err(io_err("invalid src address")))))
        };

        let dest = if let Some(peer) = Peer::from_message(msg.take_dstPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::HOP_DST_MULTIADDR_INVALID);
            return B(B(io.send(msg).and_then(|_| Err(io_err("invalid dest address")))))
        };

        A(future::ok(RelayOutput::HopRequest(RelayHopRequest {
            stream: io,
            dest,
        })))
    }

    /// STOP message handling (we are a destination).
    fn on_stop<C>(self, mut msg: CircuitRelay, io: Io<C>) -> impl Future<Item = RelayOutput<C>, Error = io::Error>
    where
        C: AsyncRead + AsyncWrite + 'static,
    {
        let from = if let Some(peer) = Peer::from_message(msg.take_srcPeer()) {
            peer
        } else {
            let msg = status(CircuitRelay_Status::HOP_SRC_MULTIADDR_INVALID);
            return B(io.send(msg).and_then(|_| Err(io_err("invalid src address"))));
        };

        A(future::ok(RelayOutput::DestinationRequest(RelayDestinationRequest {
            stream: io,
            from,
        })))
    }
}

/// Generates a message that requests proxying.
fn hop_message(destination: &Peer) -> CircuitRelay {
    let mut msg = CircuitRelay::new();
    msg.set_field_type(CircuitRelay_Type::HOP);

    let mut dest = CircuitRelay_Peer::new();
    dest.set_id(destination.id.as_bytes().to_vec());
    for a in &destination.addrs {
        dest.mut_addrs().push(a.to_bytes())
    }
    msg.set_dstPeer(dest);

    msg
}

/// Generates a STOP message to send to a destination.
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

/// Builds a message that refuses a HOP request.
fn deny_message(status: CircuitRelay_Status) -> CircuitRelay {
    let mut msg = CircuitRelay::new();
    msg.set_field_type(CircuitRelay_Type::STATUS);  // TODO: is this correct?
    msg.set_code(status);
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

