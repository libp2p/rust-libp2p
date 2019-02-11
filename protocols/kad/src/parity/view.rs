// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::addresses::Addresses;
use crate::kbucket::{self, KBucketsPeerId, KBucketsTable};
use crate::parity::Namespace;
use fnv::FnvHashMap;
use libp2p_core::{Multiaddr, PeerId, swarm::ConnectedPoint};
use std::{collections::hash_map::Entry, iter, time::Duration};

/// Stores the view of the network from the point of view of Kademlia.
pub struct View {
    /// Contains the list of nodes in our view.
    ///
    /// These are the nodes that we try to maintain information about, that can be returned when
    /// answering RPC queries.
    ///
    /// The k-buckets should only contain nodes which we think are reachable. In other words, we
    /// shouldn't add nodes just because we think they exist, but only after we have successfully
    /// reached them in the past.
    /// If we fail to reach a node, it should be removed from the k-buckets.
    kbuckets: KBucketsTable<(Namespace, PeerId), ()>,

    /// Information about nodes. Should only ever contain entries that have either `in_view`, or
    /// `connected`, or both, set to `Some`.
    peer_info: FnvHashMap<PeerId, PeerInfo>,
}

/// Information about a peer.
#[derive(Debug, Clone)]
struct PeerInfo {
    /// If `Some`, the node is in our view.
    ///
    /// Kept in sync with `kbuckets`; in other words, nodes whose have this as `Some` are also in
    /// `kbuckets` and vice-versa.
    /// We use a separate data structure from the k-buckets because the k-buckets are ordered by
    /// namespace.
    in_view: Option<PeerInfoInView>,

    /// If `Some`, we're connected to this node.
    connected: Option<PeerInfoConnected>,
}

/// Information about a peer that we are connected to.
#[derive(Debug, Clone)]
enum PeerInfoConnected {
    /// We are connected as a dialer, and therefore know the address of the remote that we are
    /// connected to.
    Dialer(Multiaddr),
    /// We are connected as a listener, and therefore have no information.
    Listener,
}

impl From<ConnectedPoint> for PeerInfoConnected {
    fn from(pt: ConnectedPoint) -> PeerInfoConnected {
        match pt {
            ConnectedPoint::Dialer { address } => PeerInfoConnected::Dialer(address),
            ConnectedPoint::Listener { .. } => PeerInfoConnected::Listener,
        }
    }
}

/// Information about a peer when it is in the view.
#[derive(Debug, Clone)]
struct PeerInfoInView {
    /// List of addresses that we will send to remotes.
    addresses: Addresses,

    /// Namespace this node belongs to. A peer can only belong to one namespace, otherwise the
    /// Kademlia-with-prefix system doesn't work properly anymore.
    namespace: Namespace,
}

/// Outcome of modifying the view.
// TODO: remove?
#[derive(Debug, Clone)]
pub struct ViewModifOutcome {
    /// If an entry has been added to the view, returns it.
    pub added: Option<(Namespace, PeerId)>,
    /// If an entry has been removed from the view, returns it.
    pub removed: Option<(Namespace, PeerId)>,
}

impl ViewModifOutcome {
    /// Builds an empty `ViewModifOutcome`.
    pub fn empty() -> ViewModifOutcome {
        ViewModifOutcome {
            added: None,
            removed: None,
        }
    }
}

impl View {
    /// Builds a new empty `View`.
    pub fn new(local_namespace: Namespace, local_peer_id: PeerId) -> View {
        View {
            // TODO: constant
            kbuckets: KBucketsTable::new((local_namespace, local_peer_id), Duration::from_secs(60)),
            peer_info: FnvHashMap::default(),
        }
    }

    /// Returns an object that allows controlling the state of the given node within the view.
    pub fn node_mut<'a>(&'a mut self, peer_id: &'a PeerId) -> NodeRefMut<'a> {
        NodeRefMut {
            view: self,
            peer_id,
        }
    }

    /// Returns the identifier of the local node.
    pub fn my_id(&self) -> &(Namespace, PeerId) {
        self.kbuckets.my_id()
    }

    /// Finds the nodes closest to `id`, ordered by distance.
    pub fn find_closest(&mut self, id: &impl KBucketsPeerId<(Namespace, PeerId)>)
        -> impl Iterator<Item = (Namespace, PeerId)>
    {
        self.kbuckets.find_closest(id)
    }
}

/// Reference to the state of a node within the view.
pub struct NodeRefMut<'a> {
    /// The view we're modifying.
    view: &'a mut View,
    /// Peer that is concerned.
    peer_id: &'a PeerId,
}

impl<'a> NodeRefMut<'a> {
    /// Returns the `PeerId` of this node.
    pub fn peer_id(&self) -> &'a PeerId {
        self.peer_id
    }

    /// Returns the namespace of the node, if it is in the view.
    pub fn namespace(&self, id: &PeerId) -> Option<&Namespace> {
        self.view.peer_info
            .get(self.peer_id)
            .and_then(|i| i.in_view.as_ref())
            .map(|p| &p.namespace)
    }

    /// Returns the addresses of the node in our view. Returns an empty iterator if the node is not
    /// in our view.
    pub fn addresses_in_view(&self) -> impl Iterator<Item = &Multiaddr> {
        self.view.peer_info
            .get(self.peer_id)
            .and_then(|i| i.in_view.as_ref())
            .into_iter()
            .flat_map(|e| e.addresses.iter())
    }

    /// Dispatch depending on the connected state of the node.
    pub fn into_connected_state(self) -> NodeRefMutConnecState<'a> {
        if self.view.peer_info.get(&self.peer_id).map(|p| p.connected.is_some()).unwrap_or(false) {
            NodeRefMutConnecState::Connected(NodeRefMutConnected {
                view: self.view,
                peer_id: self.peer_id,
            })
        } else {
            NodeRefMutConnecState::Disconnected(NodeRefMutDisconnected {
                view: self.view,
                peer_id: self.peer_id,
            })
        }
    }

    /// Returns `Some` if the node is connected.
    pub fn into_connected(self) -> Option<NodeRefMutConnected<'a>> {
        if let NodeRefMutConnecState::Connected(node) = self.into_connected_state() {
            Some(node)
        } else {
            None
        }
    }

    /// Returns `Some` if the node is not connected.
    pub fn into_disconnected(self) -> Option<NodeRefMutDisconnected<'a>> {
        if let NodeRefMutConnecState::Disconnected(node) = self.into_connected_state() {
            Some(node)
        } else {
            None
        }
    }

    /// Dispatch depending on the whether the node is in view or not.
    pub fn into_view_state(self) -> NodeRefMutViewState<'a> {
        if self.view.peer_info.get(&self.peer_id).map(|p| p.in_view.is_some()).unwrap_or(false) {
            NodeRefMutViewState::InView(NodeRefMutInView {
                view: self.view,
                peer_id: self.peer_id,
            })
        } else {
            NodeRefMutViewState::NotInView(NodeRefMutNotInView {
                view: self.view,
                peer_id: self.peer_id,
            })
        }
    }

    /// Returns `Some` if the node is in view.
    pub fn into_in_view(self) -> Option<NodeRefMutInView<'a>> {
        if let NodeRefMutViewState::InView(node) = self.into_view_state() {
            Some(node)
        } else {
            None
        }
    }

    /// Returns `Some` if the node is not in view.
    pub fn into_not_in_view(self) -> Option<NodeRefMutNotInView<'a>> {
        if let NodeRefMutViewState::NotInView(node) = self.into_view_state() {
            Some(node)
        } else {
            None
        }
    }
}

/// Reference to the state of a node within the view.
pub enum NodeRefMutViewState<'a> {
    /// This node is in the view.
    InView(NodeRefMutInView<'a>),
    /// This node is not in the view.
    NotInView(NodeRefMutNotInView<'a>),
}

/// Reference to the state of a node that we know is in the view.
pub struct NodeRefMutInView<'a> {
    /// The view we're modifying.
    view: &'a mut View,
    /// Peer that is concerned.
    peer_id: &'a PeerId,
}

impl<'a> NodeRefMutInView<'a> {
    fn view_state(&self) -> &PeerInfoInView {
        self.view.peer_info
            .get(self.peer_id)
            .and_then(|p| p.in_view.as_ref())
            .expect("We can only construct a NodeRefMutInView if peer_info contains the id; QED")
    }

    fn view_state_mut(&mut self) -> &mut PeerInfoInView {
        self.view.peer_info
            .get_mut(self.peer_id)
            .and_then(|p| p.in_view.as_mut())
            .expect("We can only construct a NodeRefMutInView if peer_info contains the id; QED")
    }

    /// Returns the namespace of the node.
    pub fn namespace(&self) -> &Namespace {
        &self.view_state().namespace
    }

    /// Returns the known addresses of the node.
    pub fn addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.view_state().addresses.iter()
    }

    /// Stores a known address in the view. The address is deemed trusted, and can be sent to
    /// other peers.
    pub fn add_address(&mut self, addr: &Multiaddr) {
        self.add_addresses(iter::once(addr));
    }

    /// Same as `add_address`, adds multiple addresses at once.
    pub fn add_addresses<'s>(&mut self, addrs: impl IntoIterator<Item = &'s Multiaddr>) {
        let mut info = self.view_state_mut();
        let is_connected = info.addresses.is_connected();
        for addr in addrs {
            if is_connected {
                info.addresses.insert_connected(addr.clone());
            } else {
                info.addresses.insert_not_connected(addr.clone());
            }
        }
    }

    /// Mark the given address as unreachable for the given peer.
    ///
    /// If this address was contained in the view for the given peer, removes it and returns
    /// `true`. Otherwise, has no effect and returns `false`.
    pub fn remove_address(&mut self, addr: &Multiaddr) -> bool {
        self.view_state_mut().addresses.remove_addr(addr)
    }
}

/// Reference to the state of a node that we know is not in the view.
pub struct NodeRefMutNotInView<'a> {
    /// The view we're modifying.
    view: &'a mut View,
    /// Peer that is concerned.
    peer_id: &'a PeerId,
}

impl<'a> NodeRefMutNotInView<'a> {
    /// Report the namespace of the peer.
    ///
    /// Right now, has no effect if the namespace of this peer was already known.
    // TODO: right now we don't update the namespace, but would it make sense to
    //       allow that? does this have nasty side-effects?
    pub fn set_namespace(mut self, namespace: Namespace) -> NodeRefMutViewState<'a> {
        println!("I am {:?}, got report for {:?} => {:?}", self.view.kbuckets.my_id(), self.peer_id, namespace);

        let mut initial_addrs = Addresses::default();
        if let Some(PeerInfoConnected::Dialer(addr)) =
            self.view.peer_info.get(&self.peer_id).and_then(|p| p.connected.as_ref())
        {
            initial_addrs.insert_connected(addr.clone());
        }

        // TODO: analyze result of set_connected
        match self.view.kbuckets.set_connected(&(namespace, self.peer_id.clone())) {
            kbucket::Update::Added => {},
            kbucket::Update::Updated => unreachable!("Whenever we insert an entry in k-buckets \
                we also add it to peer_info; therefore it's not possible that an entry is in \
                k-buckets but not in peer_info; QED"),
            kbucket::Update::Pending(_) => unimplemented!(),
            kbucket::Update::Discarded => unimplemented!(),
            kbucket::Update::FailSelfUpdate => (),
            _ => ()
        }

        self.view.peer_info.entry(self.peer_id.clone())
            .or_insert_with(|| PeerInfo { connected: None, in_view: None })
            .in_view = Some(PeerInfoInView {
                addresses: initial_addrs,
                namespace,
            });

        NodeRefMutViewState::InView(NodeRefMutInView {
            view: self.view,
            peer_id: self.peer_id,
        })
    }
}

/// Reference to the state of a node within the view.
pub enum NodeRefMutConnecState<'a> {
    /// We are connected to this node.
    Connected(NodeRefMutConnected<'a>),
    /// We are not connected to this node.
    Disconnected(NodeRefMutDisconnected<'a>),
}

/// Reference to the state of a node within the view that we know is connected.
pub struct NodeRefMutConnected<'a> {
    /// The view we're modifying.
    view: &'a mut View,
    /// Peer that is concerned.
    peer_id: &'a PeerId,
}

impl<'a> NodeRefMutConnected<'a> {
    /// Mark the given peer as connected.
    pub fn replace_connected_point(&mut self, new_endpoint: ConnectedPoint) {
        let mut info = self.view.peer_info
            .get_mut(self.peer_id)
            .expect("We can only construct a NodeRefMutConnected if peer_info contains the id; QED");

        // If the node is in our k-buckets, update its addresses.
        if let Some(in_view) = info.in_view.as_mut() {
            debug_assert!(in_view.addresses.is_connected() || in_view.addresses.is_empty());

            if let ConnectedPoint::Dialer { ref address } = new_endpoint {
                in_view.addresses.insert_connected(address.clone());
            }

            self.view.kbuckets.set_connected(&(in_view.namespace, self.peer_id.clone()));
        }

        // Update `connected_peers`.
        debug_assert!(info.connected.is_some());
        info.connected = Some(new_endpoint.into());
    }

    /// Updates the view to set the given peer as disconnected.
    pub fn set_disconnected(self) -> NodeRefMutDisconnected<'a> {
        match self.view.peer_info.entry(self.peer_id.clone()) {
            Entry::Vacant(_) => unreachable!("We can construct a NodeRefMutConnected only if \
                                              there's an element in peer_info"),
            Entry::Occupied(mut entry) => {
                entry.get_mut().connected = None;
                if let Some(in_view) = entry.get_mut().in_view.as_mut() {
                    in_view.addresses.set_all_disconnected();
                    self.view.kbuckets.set_disconnected(&(in_view.namespace, self.peer_id.clone()));
                } else {
                    entry.remove();
                }
            }
        }

        NodeRefMutDisconnected {
            view: self.view,
            peer_id: self.peer_id,
        }
    }
}

/// Reference to the state of a node within the view that we know is not connected.
pub struct NodeRefMutDisconnected<'a> {
    /// The view we're modifying.
    view: &'a mut View,
    /// Peer that is concerned.
    peer_id: &'a PeerId,
}

impl<'a> NodeRefMutDisconnected<'a> {
    /// Mark the given peer as connected. If the node was already in the view, this updates its
    /// position.
    pub fn set_connected(self, endpoint: ConnectedPoint) -> NodeRefMutConnected<'a> {
        let mut info = self.view.peer_info.entry(self.peer_id.clone())
            .or_insert_with(|| PeerInfo { in_view: None, connected: None });

        if let Some(in_view) = info.in_view.as_mut() {
            debug_assert!(!in_view.addresses.is_connected());

            if let ConnectedPoint::Dialer { ref address } = endpoint {
                in_view.addresses.insert_connected(address.clone());
            }

            self.view.kbuckets.set_connected(&(in_view.namespace, self.peer_id.clone()));
        }

        debug_assert!(info.connected.is_none());
        info.connected = Some(endpoint.into());

        NodeRefMutConnected {
            view: self.view,
            peer_id: self.peer_id,
        }
    }
}
