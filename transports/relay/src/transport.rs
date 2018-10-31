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

use crate::{
    error::RelayError,
    message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Type},
    protocol,
    utility::{Peer, RelayAddr}
};
use futures::{stream, prelude::*};
use libp2p_core::transport::Dialer;
use log::{debug, info, trace};
use multiaddr::Multiaddr;
use peerstore::{PeerAccess, PeerId, Peerstore};
use rand::{self, Rng};
use std::{io, iter::FromIterator, ops::Deref, sync::Arc};
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Clone)]
pub struct RelayDialer<T, P> {
    my_id: PeerId,
    dialer: T,
    peers: P,
    relays: Arc<Vec<PeerId>>
}

impl<T, P, S> Dialer for RelayDialer<T, P>
where
    T: Dialer + Send + Clone + 'static,
    T::Outbound: Send,
    T::Output: AsyncRead + AsyncWrite + Send,
    T::Error: Send,
    P: Deref<Target=S> + Clone + 'static,
    S: 'static,
    for<'a> &'a S: Peerstore
{
    type Output = T::Output;
    type Error = RelayError<T::Error, io::Error>;
    type Outbound = Box<Future<Item=Self::Output, Error=Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
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
}

impl<T, P, S> RelayDialer<T, P>
where
    T: Dialer + Clone + 'static,
    T::Outbound: Send,
    T::Error: Send,
    T::Output: AsyncRead + AsyncWrite + Send,
    P: Deref<Target=S> + Clone + 'static,
    for<'a> &'a S: Peerstore
{
    /// Create a new relay transport.
    ///
    /// This transport uses a static set of relays and will not attempt
    /// any relay discovery.
    pub fn new<R>(my_id: PeerId, dialer: T, peers: P, relays: R) -> Self
    where
        R: IntoIterator<Item = PeerId>,
    {
        RelayDialer {
            my_id,
            dialer,
            peers,
            relays: Arc::new(Vec::from_iter(relays)),
        }
    }

    // Relay to destination over any available relay node.
    fn relay_to(self, destination: &Peer) -> Result<impl Future<Item=T::Output, Error=RelayError<T::Error, io::Error>>, Self> {
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
                    Err(RelayError::NoRelayFor(dest_peer))
                }
            });
        Ok(future)
    }

    // Relay to destination via the given peer.
    fn relay_via(self, relay: &Peer, destination: &Peer) -> Result<impl Future<Item=T::Output, Error=RelayError<T::Error, io::Error>>, Self> {
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
        let dialer = self.dialer.with_dialer_upgrade(protocol::Source(message));
        let future = stream::iter_ok(addresses.into_iter())
            .filter_map(move |addr| dialer.clone().dial(addr).ok())
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
                    Err(RelayError::Message("failed to dial to relay"))
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
