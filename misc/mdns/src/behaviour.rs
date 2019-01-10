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

use crate::service::{MdnsService, MdnsPacket};
use futures::prelude::*;
use libp2p_core::protocols_handler::{DummyProtocolsHandler, ProtocolsHandler};
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{Multiaddr, PeerId, multiaddr::Protocol, topology::MemoryTopology, topology::Topology};
use smallvec::SmallVec;
use std::{fmt, io, iter, marker::PhantomData, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use void::{self, Void};

/// A `NetworkBehaviour` for mDNS. Automatically discovers peers on the local network and adds
/// them to the topology.
pub struct Mdns<TSubstream> {
    /// The inner service.
    service: MdnsService,

    /// If `Some`, then we automatically connect to nodes we discover and this is the list of nodes
    /// to connect to. Drained in `poll()`.
    /// If `None`, then we don't automatically connect.
    to_connect_to: Option<SmallVec<[PeerId; 8]>>,

    /// Marker to pin the generic.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Mdns<TSubstream> {
    /// Builds a new `Mdns` behaviour.
    pub fn new() -> io::Result<Mdns<TSubstream>> {
        Ok(Mdns {
            service: MdnsService::new()?,
            to_connect_to: Some(SmallVec::new()),
            marker: PhantomData,
        })
    }
}

/// Trait that must be implemented on the network topology for it to be usable with `Mdns`.
pub trait MdnsTopology: Topology {
    /// Adds an address discovered by mDNS.
    ///
    /// Will never be called with the local peer ID.
    fn add_mdns_discovered_address(&mut self, peer: PeerId, addr: Multiaddr);
}

impl MdnsTopology for MemoryTopology {
    #[inline]
    fn add_mdns_discovered_address(&mut self, peer: PeerId, addr: Multiaddr) {
        self.add_address(peer, addr)
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for Mdns<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
    TTopology: MdnsTopology,
{
    type ProtocolsHandler = DummyProtocolsHandler<TSubstream>;
    type OutEvent = Void;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        DummyProtocolsHandler::default()
    }

    fn inject_connected(&mut self, _: PeerId, _: ConnectedPoint) {}

    fn inject_disconnected(&mut self, _: &PeerId, _: ConnectedPoint) {}

    fn inject_node_event(
        &mut self,
        _: PeerId,
        _ev: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        void::unreachable(_ev)
    }

    fn poll(
        &mut self,
        params: &mut PollParameters<TTopology>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        loop {
            if let Some(ref mut to_connect_to) = self.to_connect_to {
                if !to_connect_to.is_empty() {
                    let peer_id = to_connect_to.remove(0);
                    return Async::Ready(NetworkBehaviourAction::DialPeer { peer_id });
                } else {
                    to_connect_to.shrink_to_fit();
                }
            }

            let event = match self.service.poll() {
                Async::Ready(ev) => ev,
                Async::NotReady => return Async::NotReady,
            };

            match event {
                MdnsPacket::Query(query) => {
                    let _ = query.respond(
                        params.local_peer_id().clone(),
                        params.listened_addresses().cloned(),
                        Duration::from_secs(5 * 60)
                    );
                },
                MdnsPacket::Response(response) => {
                    // We perform a call to `nat_traversal()` with the address we observe the
                    // remote as and the address they listen on.
                    let obs_ip = Protocol::from(response.remote_addr().ip());
                    let obs_port = Protocol::Udp(response.remote_addr().port());
                    let observed: Multiaddr = iter::once(obs_ip)
                        .chain(iter::once(obs_port))
                        .collect();

                    for peer in response.discovered_peers() {
                        if peer.id() == params.local_peer_id() {
                            continue;
                        }

                        for addr in peer.addresses() {
                            if let Some(new_addr) = params.nat_traversal(&addr, &observed) {
                                params.topology().add_mdns_discovered_address(peer.id().clone(), new_addr);
                            }

                            params.topology().add_mdns_discovered_address(peer.id().clone(), addr);
                        }

                        if let Some(ref mut to_connect_to) = self.to_connect_to {
                            to_connect_to.push(peer.id().clone());
                        }
                    }
                },
                MdnsPacket::ServiceDiscovery(disc) => {
                    disc.respond(Duration::from_secs(5 * 60));
                },
            }
        }
    }
}

impl<TSubstream> fmt::Debug for Mdns<TSubstream> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Mdns")
            .field("service", &self.service)
            .finish()
    }
}
