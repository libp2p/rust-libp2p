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

use core::{PeerId, Transport};
use futures::{stream, prelude::*};
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Type};
use multiaddr::Multiaddr;
use protocol;
use std::io;
use tokio_io::{AsyncRead, AsyncWrite};
use utility::{io_err, Peer, RelayAddr};

/// Implementation of `Transport` that supports addresses of the form
/// `<addr1>/p2p-circuit/<addr2>`.
///
/// `<addr1>` is the address of a node that should act as a relay, and `<addr2>` is the address
/// of the destination, that the relay should dial.
#[derive(Debug, Clone)]
pub struct RelayTransport<T> {
    /// Id of the local node.
    my_id: PeerId,
    /// Transport to use to.
    transport: T,
}

impl<T> Transport for RelayTransport<T>
where
    T: Transport + Send + Clone + 'static,
    T::Dial: Send,
    T::Listener: Send,
    T::ListenerUpgrade: Send,
    T::Output: AsyncRead + AsyncWrite + Send,
{
    type Output = T::Output;
    type Listener = Box<Stream<Item=(Self::ListenerUpgrade, Multiaddr), Error=io::Error> + Send>;
    type ListenerUpgrade = Box<Future<Item=Self::Output, Error=io::Error> + Send>;
    type Dial = Box<Future<Item=Self::Output, Error=io::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        Err((self, addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match RelayAddr::parse(&addr) {
            RelayAddr::Malformed => {
                debug!("malformed address: {}", addr);
                return Err((self, addr));
            }
            RelayAddr::Multihop => {
                debug!("multihop address: {}", addr);
                return Err((self, addr));
            }
            RelayAddr::Address { relay, dest } => {
                let f = self.relay_via(&relay, &dest).map_err(|this| (this, addr))?;
                Ok(Box::new(f))
            }
        }
    }

    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

impl<T> RelayTransport<T>
where
    T: Transport + Clone + 'static,
    T::Dial: Send,
    T::Listener: Send,
    T::ListenerUpgrade: Send,
    T::Output: AsyncRead + AsyncWrite + Send,
{
    /// Create a new relay transport.
    ///
    /// This transport uses a static set of relays and will not attempt
    /// any relay discovery.
    pub fn new(my_id: PeerId, transport: T) -> Self {
        RelayTransport {
            my_id,
            transport,
        }
    }

    // Relay to destination via the given peer.
    fn relay_via(self, relay: &Peer, destination: &Peer) -> Result<impl Future<Item=T::Output, Error=io::Error>, Self> {
        trace!("relay_via {:?} to {:?}", relay.id, destination.id);

        // no relay address => bail out
        if relay.addrs.is_empty() {
            info!("no available address for relay: {:?}", relay.id);
            return Err(self);
        }

        let relay = relay.clone();
        let message = self.hop_message(destination);
        let transport = self.transport.with_upgrade(protocol::Source(message));
        let future = stream::iter_ok(relay.addrs.clone().into_iter())
            .filter_map(move |addr| transport.clone().dial(addr).ok())
            .and_then(|dial| dial)
            .then(|result| Ok(result.ok()))
            .filter_map(|result| result)
            .into_future()
            .map_err(|(err, _stream)| err)
            .and_then(move |(ok, _stream)| match ok {
                Some(out) => {
                    debug!("connected");
                    Ok(out)
                }
                None => {
                    info!("failed to dial to {:?}", relay.id);
                    Err(io_err(format!("failed to dial to relay {:?}", relay.id)))
                }
            });
        Ok(future)
    }

    fn hop_message(&self, destination: &Peer) -> CircuitRelay {
        let mut msg = CircuitRelay::new();
        msg.set_field_type(CircuitRelay_Type::HOP);

        let mut from = CircuitRelay_Peer::new();
        from.set_id(self.my_id.as_bytes().to_vec());
        // TODO: fill from.mut_addrs()
        msg.set_srcPeer(from);

        let mut dest = CircuitRelay_Peer::new();
        dest.set_id(destination.id.as_bytes().to_vec());
        for a in &destination.addrs {
            dest.mut_addrs().push(a.to_bytes())
        }
        msg.set_dstPeer(dest);

        msg
    }
}
