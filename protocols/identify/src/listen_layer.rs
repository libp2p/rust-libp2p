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

use futures::prelude::*;
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, Multiaddr, PeerId};
use smallvec::SmallVec;
use std::collections::HashMap;
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;
use {IdentifyListenHandler, IdentifyInfo, IdentifySenderFuture};

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct IdentifyListen<TSubstream> {
    /// Information to send back to remotes.
    send_back_info: IdentifyInfo,
    /// For each peer we're connected to, the observed address to send back to it.
    observed_addresses: HashMap<PeerId, Multiaddr>,
    /// List of futures that send back information back to remotes.
    futures: SmallVec<[IdentifySenderFuture<TSubstream>; 4]>,
}

impl<TSubstream> IdentifyListen<TSubstream> {
    /// Creates a `IdentifyListen`.
    pub fn new(info: IdentifyInfo) -> Self {
        IdentifyListen {
            send_back_info: info,
            observed_addresses: HashMap::new(),
            futures: SmallVec::new(),
        }
    }

    /// Gets the information that is sent back to remotes.
    #[inline]
    pub fn infos(&self) -> &IdentifyInfo {
        &self.send_back_info
    }

    /// Modifies the information to send back to remotes.
    #[inline]
    pub fn infos_mut(&mut self) -> &mut IdentifyInfo {
        &mut self.send_back_info
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for IdentifyListen<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = IdentifyListenHandler<TSubstream>;
    type OutEvent = Void;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        IdentifyListenHandler::new()
    }

    fn inject_connected(&mut self, peer_id: PeerId, endpoint: ConnectedPoint) {
        let observed = match endpoint {
            ConnectedPoint::Dialer { address } => address,
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr,
        };

        self.observed_addresses.insert(peer_id, observed);
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId, _: ConnectedPoint) {
        self.observed_addresses.remove(peer_id);
    }

    fn inject_node_event(
        &mut self,
        peer_id: PeerId,
        sender: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        let observed = self.observed_addresses.get(&peer_id)
            .expect("We only receive events from nodes we're connected to ; we insert into the \
                     hashmap when we connect to a node and remove only when we disconnect; QED");
        let future = sender.send(self.send_back_info.clone(), &observed);
        self.futures.push(future);
    }

    fn poll(
        &mut self,
        _: &mut PollParameters<TTopology>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Removes each future one by one, and pushes them back if they're not ready.
        for n in (0..self.futures.len()).rev() {
            let mut future = self.futures.swap_remove(n);
            match future.poll() {
                Ok(Async::Ready(())) => {}
                Ok(Async::NotReady) => self.futures.push(future),
                Err(_) => {},
            }
        }

        Async::NotReady
    }
}
