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
use libp2p_core::{Multiaddr, PeerId, topology::MemoryTopology};
use std::{io, marker::PhantomData, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use void::{self, Void};

/// A `NetworkBehaviour` for mDNS. Automatically discovers peers on the local network and adds
/// them to the topology.
// TODO: #[derive(Debug)]
pub struct Mdns<TSubstream> {
    /// The inner service.
    service: MdnsService,
    /// Marker to pin the generic.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Mdns<TSubstream> {
    /// Builds a new `Mdns` behaviour.
    pub fn new() -> io::Result<Mdns<TSubstream>> {
        Ok(Mdns {
            service: MdnsService::new()?,
            marker: PhantomData,
        })
    }
}

/// Trait that must be implemented on the network topology for it to be usable with `Mdns`.
pub trait MdnsTopology {
    /// Adds an address discovered by mDNS.
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
                    for peer in response.discovered_peers() {
                        for addr in peer.addresses() {
                            // TODO: call nat_traversal on the address
                            params.topology().add_mdns_discovered_address(peer.id().clone(), addr);
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
