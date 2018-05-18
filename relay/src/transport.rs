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

use core::Transport;
use futures::{stream, prelude::*};
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Type};
use multiaddr::Multiaddr;
use peerstore::{PeerAccess, PeerId, Peerstore};
use protocol;
use rand::{self, Rng};
use std::{io, iter::FromIterator, ops::Deref, sync::Arc};
use tokio_io::{AsyncRead, AsyncWrite};
use utility::{io_err, Peer, RelayAddr};

#[derive(Debug, Clone)]
pub struct RelayTransport<T, P> {
    my_id: PeerId,
    transport: T,
    peers: P,
    relays: Arc<Vec<PeerId>>
}

impl<T, P, S> Transport for RelayTransport<T, P>
where
    T: Transport + Clone + 'static,
    T::Output: AsyncRead + AsyncWrite,
    P: Deref<Target=S> + Clone + 'static,
    S: 'static,
    for<'a> &'a S: Peerstore
{
    type Output = T::Output;
    type Listener = Box<Stream<Item=Self::ListenerUpgrade, Error=io::Error>>;
    type ListenerUpgrade = Box<Future<Item=(Self::Output, Multiaddr), Error=io::Error>>;
    type Dial = Box<Future<Item=(Self::Output, Multiaddr), Error=io::Error>>;

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
                if let Some(ref r) = relay {
                    let f = self.relay_via(r, &dest).map_err(|this| (this, addr))?;
                    Ok(Box::new(f))
                } else {
                    let f = self.relay_to(&dest).map_err(|this| (this, addr))?;
                    Ok(Box::new(f))
                }
            }
        }
    }

    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

impl<T, P, S> RelayTransport<T, P>
where
    T: Transport + Clone + 'static,
    T::Output: AsyncRead + AsyncWrite,
    P: Deref<Target=S> + Clone + 'static,
    for<'a> &'a S: Peerstore
{
    /// Create a new relay transport.
    ///
    /// This transport uses a static set of relays and will not attempt
    /// any relay discovery.
    pub fn new<R>(my_id: PeerId, transport: T, peers: P, relays: R) -> Self
    where
        R: IntoIterator<Item = PeerId>,
    {
        RelayTransport {
            my_id,
            transport,
            peers,
            relays: Arc::new(Vec::from_iter(relays)),
        }
    }

    // Relay to destination over any available relay node.
    fn relay_to(self, destination: &Peer) -> Result<impl Future<Item=(T::Output, Multiaddr), Error=io::Error>, Self> {
        trace!("relay_to {:?}", destination.id);
        let mut dials = Vec::new();
        for relay in &*self.relays {
            let relay_peer = Peer {
                id: relay.clone(),
                addrs: Vec::new(),
            };
            if let Ok(dial) = self.clone().relay_via(&relay_peer, destination) {
                dials.push(dial)
            }
        }

        if dials.is_empty() {
            info!("no relay available for {:?}", destination.id);
            return Err(self);
        }

        // Try one relay after another and stick to the first working one.
        rand::thread_rng().shuffle(&mut dials); // randomise to spread load
        let dest_peer = destination.id.clone();
        let future = stream::iter_ok(dials.into_iter())
            .and_then(|dial| dial)
            .then(|result| Ok(result.ok()))
            .filter_map(|result| result)
            .into_future()
            .map_err(|(err, _stream)| err)
            .and_then(move |(ok, _stream)| {
                if let Some(out) = ok {
                    Ok(out)
                } else {
                    Err(io_err(format!("no relay for {:?}", dest_peer)))
                }
            });
        Ok(future)
    }

    // Relay to destination via the given peer.
    fn relay_via(self, relay: &Peer, destination: &Peer) -> Result<impl Future<Item=(T::Output, Multiaddr), Error=io::Error>, Self> {
        trace!("relay_via {:?} to {:?}", relay.id, destination.id);
        let mut addresses = Vec::new();

        if relay.addrs.is_empty() {
            // try all known relay addresses
            if let Some(peer) = self.peers.peer(&relay.id) {
                addresses.extend(peer.addrs())
            }
        } else {
            // use only specific relay addresses
            addresses.extend(relay.addrs.iter().cloned())
        }

        // no relay address => bail out
        if addresses.is_empty() {
            info!("no available address for relay: {:?}", relay.id);
            return Err(self);
        }

        let relay = relay.clone();
        let message = self.hop_message(destination);
        let transport = self.transport.with_upgrade(protocol::Source(message));
        let future = stream::iter_ok(addresses.into_iter())
            .filter_map(move |addr| transport.clone().dial(addr).ok())
            .and_then(|dial| dial)
            .then(|result| Ok(result.ok()))
            .filter_map(|result| result)
            .into_future()
            .map_err(|(err, _stream)| err)
            .and_then(move |(ok, _stream)| match ok {
                Some((out, addr)) => {
                    debug!("connected to {:?}", addr);
                    Ok((out, addr))
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
        if let Some(me) = self.peers.peer(&self.my_id) {
            for a in me.addrs() {
                from.mut_addrs().push(a.to_bytes())
            }
        }
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
