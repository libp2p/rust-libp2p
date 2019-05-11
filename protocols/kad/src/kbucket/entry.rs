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

//! Entry API for quering and modifying entries in a `KBucketsTable`.

use super::*;

/// A cloned, immutable view of an entry in a bucket.
#[derive(Clone, Debug)]
pub struct EntryView<TPeerId, TVal> {
    /// The key of the entry.
    pub key: Key<TPeerId>,
    /// The value of the entry.
    pub value: TVal,
    /// Whether the peer identified by the key is currently considered connected.
    pub connected: bool
}

impl<TPeerId, TVal> AsRef<Key<TPeerId>> for EntryView<TPeerId, TVal> {
    fn as_ref(&self) -> &Key<TPeerId> {
        &self.key
    }
}

/// Represents an entry or a potential entry in the k-buckets.
pub enum Entry<'a, TPeerId, TVal> {
    /// Entry in a k-bucket that we're connected to.
    InKbucketConnected(EntryInKbucketConn<'a, TPeerId, TVal>),
    /// Entry pending waiting for a free slot to enter a k-bucket. We're connected to it.
    InKbucketConnectedPending(EntryInKbucketConnPending<'a, TPeerId, TVal>),
    /// Entry in a k-bucket but that we're not connected to.
    InKbucketDisconnected(EntryInKbucketDisc<'a, TPeerId, TVal>),
    /// Entry pending waiting for a free slot to enter a k-bucket. We're not connected to it.
    InKbucketDisconnectedPending(EntryInKbucketDiscPending<'a, TPeerId, TVal>),
    /// Entry is not present in any k-bucket.
    NotInKbucket(EntryNotInKbucket<'a, TPeerId, TVal>),
    /// Entry is the local peer ID.
    SelfEntry,
}

impl<'a, TPeerId, TVal> Entry<'a, TPeerId, TVal>
where
    TPeerId: Clone,
{
    /// Returns an object containing the state of the given entry.
    pub(super) fn new(parent: &'a mut KBucketsTable<TPeerId, TVal>, peer_id: &'a Key<TPeerId>)
        -> Entry<'a, TPeerId, TVal>
    {
        let bucket = if let Some(bucket) = parent.bucket_mut(peer_id) {
            bucket
        } else {
            return Entry::SelfEntry;
        };

        // Update the pending node state.
        // TODO: must be reported to the user somehow, in a non-annoying API
        bucket.apply_pending();

        // Try to find the node in the bucket.
        if let Some(pos) = bucket.nodes.iter().position(|p| p.id == *peer_id) {
            if bucket.first_connected_pos.map_or(false, |i| pos >= i) {
                Entry::InKbucketConnected(EntryInKbucketConn { parent, peer_id })
            } else {
                Entry::InKbucketDisconnected(EntryInKbucketDisc { parent, peer_id })
            }
        } else if bucket.pending_node.as_ref().map(|p| p.node.id == *peer_id).unwrap_or(false) {
            // Node is pending.
            if bucket.pending_node.as_ref().map(|p| p.connected).unwrap_or(false) {
                Entry::InKbucketConnectedPending(EntryInKbucketConnPending { parent, peer_id })
            } else {
                Entry::InKbucketDisconnectedPending(EntryInKbucketDiscPending { parent, peer_id })
            }
        } else {
            Entry::NotInKbucket(EntryNotInKbucket { parent, peer_id })
        }
    }


    /// Creates a cloned, immutable view of the entry, if it is contained in a bucket.
    ///
    /// Returns `None` if the entry is not contained in any bucket.
    pub fn view(&mut self) -> Option<EntryView<TPeerId, TVal>>
    where
        TVal: Clone
    {
        if let Some(key) = self.key().cloned() {
            if let Some(value) = self.value().cloned() {
                return Some(EntryView {
                    key, value, connected: match self {
                        Entry::InKbucketConnected(_) |
                        Entry::InKbucketConnectedPending(_) => true,
                        _ => false
                    }
                })
            }
        }
        return None
    }

    /// Returns the key of the entry.
    pub fn key(&self) -> Option<&Key<TPeerId>> {
        match self {
            Entry::InKbucketConnected(entry) => Some(entry.key()),
            Entry::InKbucketConnectedPending(entry) => Some(entry.key()),
            Entry::InKbucketDisconnected(entry) => Some(entry.key()),
            Entry::InKbucketDisconnectedPending(entry) => Some(entry.key()),
            Entry::NotInKbucket(_entry) => None,
            Entry::SelfEntry => None,
        }
    }

    /// Returns the value associated to the entry in the bucket, including if the node is pending.
    pub fn value(&mut self) -> Option<&mut TVal> {
        match self {
            Entry::InKbucketConnected(entry) => Some(entry.value()),
            Entry::InKbucketConnectedPending(entry) => Some(entry.value()),
            Entry::InKbucketDisconnected(entry) => Some(entry.value()),
            Entry::InKbucketDisconnectedPending(entry) => Some(entry.value()),
            Entry::NotInKbucket(_entry) => None,
            Entry::SelfEntry => None,
        }
    }

    /// Returns the value associated to the entry in the bucket.
    pub fn value_not_pending(&mut self) -> Option<&mut TVal> {
        match self {
            Entry::InKbucketConnected(entry) => Some(entry.value()),
            Entry::InKbucketConnectedPending(_entry) => None,
            Entry::InKbucketDisconnected(entry) => Some(entry.value()),
            Entry::InKbucketDisconnectedPending(_entry) => None,
            Entry::NotInKbucket(_entry) => None,
            Entry::SelfEntry => None,
        }
    }
}

/// Represents an entry in a k-bucket.
pub struct EntryInKbucketConn<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a Key<TPeerId>,
}

impl<'a, TPeerId, TVal> EntryInKbucketConn<'a, TPeerId, TVal>
where
    TPeerId: Clone,
{
    pub fn key(&self) -> &Key<TPeerId> {
        self.peer_id
    }

    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = self.parent.bucket_mut(&self.peer_id)
            .expect("we can only build a EntryInKbucketConn if we know of a bucket; QED");

        let peer_id = self.peer_id;
        &mut table.nodes.iter_mut()
            .find(move |p| p.id == *peer_id)
            .expect("We can only build a EntryInKbucketConn if we know that the peer is in its \
                     bucket; QED")
            .value
    }

    /// Reports that we are now disconnected from the given node.
    ///
    /// This moves the node down in its bucket. There are two possible outcomes:
    ///
    /// - Either we had a pending node which replaces the current node. `Replaced` is returned.
    /// - Or we had no pending node, and the current node is kept. `Kept` is returned.
    ///
    pub fn set_disconnected(self) -> SetDisconnectedOutcome<'a, TPeerId, TVal> {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryInKbucketConn if we know of a bucket; QED");

        let peer_id = self.peer_id;
        let pos = table.nodes.iter().position(move |elem| elem.id == *peer_id)
            .expect("we can only build a EntryInKbucketConn if the node is in its bucket; QED");
        debug_assert!(table.first_connected_pos.map_or(false, |i| i <= pos));

        // Replace the disconnected entry with the pending entry, if one exists
        // and it is considered connected.
        if let Some(pending) = table.pending_node.take() {
            if pending.connected {
                let removed = table.nodes.remove(pos);
                let replacement = pending.node.id.clone();
                // The pending entry is considered the most-recently connected, i.e.
                // pushed to the end.
                table.nodes.push(pending.node);
                return SetDisconnectedOutcome::Replaced {
                    replacement,
                    old_val: removed.value,
                }
            } else {
                table.pending_node = Some(pending);
            }
        }

        // Otherwise, there is now one less connected entry. The entry for the most-recently
        // disconnected peer precedes the entry of the first connected peer in the bucket.
        if let Some(ref mut first_connected_pos) = table.first_connected_pos {
            if pos != *first_connected_pos {
                let elem = table.nodes.remove(pos);
                table.nodes.insert(*first_connected_pos, elem);
            }
            if *first_connected_pos == table.nodes.len() - 1 {
                table.first_connected_pos = None;
            } else {
                *first_connected_pos += 1;
            }
        } else {
            let entry = table.nodes.remove(pos);
            table.nodes.push(entry);
        }

        // And return a EntryInKbucketDisc.
        debug_assert!(table.nodes.iter()
            .position(move |e| e.id == *peer_id)
            .map_or(false, |p| table.first_connected_pos.map_or(false, |i| p < i)));

        SetDisconnectedOutcome::Kept(EntryInKbucketDisc {
            parent: self.parent,
            peer_id: self.peer_id,
        })
    }
}

/// Outcome of calling `set_disconnected`.
#[must_use]
pub enum SetDisconnectedOutcome<'a, TPeerId, TVal> {
    /// Node is kept in the bucket.
    Kept(EntryInKbucketDisc<'a, TPeerId, TVal>),
    /// Node is pushed out of the bucket.
    Replaced {
        /// Node that replaced the node.
        // TODO: could be a EntryInKbucketConn, but we have borrow issues with the new peer id
        replacement: Key<TPeerId>,
        /// Value os the node that has been pushed out.
        old_val: TVal,
    },
}

/// Represents an entry waiting for a slot to be available in its k-bucket.
pub struct EntryInKbucketConnPending<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a Key<TPeerId>,
}

impl<'a, TPeerId, TVal> EntryInKbucketConnPending<'a, TPeerId, TVal>
where
    TPeerId: Clone,
{
    pub fn key(&self) -> &Key<TPeerId> {
        self.peer_id
    }

    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryInKbucketConnPending if we know of a bucket; QED");

        assert!(table.pending_node.as_ref().map(|n| &n.node.id) == Some(self.peer_id));
        &mut table.pending_node
            .as_mut()
            .expect("we can only build a EntryInKbucketConnPending if the node is pending; QED")
            .node.value
    }

    /// Reports that we are now disconnected from the given node.
    pub fn set_disconnected(self) -> EntryInKbucketDiscPending<'a, TPeerId, TVal> {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryInKbucketConnPending if we know of a bucket; QED");

        let mut pending = table.pending_node.as_mut()
            .expect("we can only build a EntryInKbucketConnPending if there's a pending node; QED");
        debug_assert!(pending.connected);
        pending.connected = false;

        EntryInKbucketDiscPending {
            parent: self.parent,
            peer_id: self.peer_id,
        }
    }
}

/// Represents an entry waiting for a slot to be available in its k-bucket.
pub struct EntryInKbucketDiscPending<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a Key<TPeerId>,
}

impl<'a, TPeerId, TVal> EntryInKbucketDiscPending<'a, TPeerId, TVal>
where
    TPeerId: Clone,
{
    pub fn key(&self) -> &Key<TPeerId> {
        self.peer_id
    }

    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryInKbucketDiscPending if we know of a bucket; QED");

        assert!(table.pending_node.as_ref().map(|n| &n.node.id) == Some(self.peer_id));
        &mut table.pending_node
            .as_mut()
            .expect("we can only build a EntryInKbucketDiscPending if the node is pending; QED")
            .node.value
    }

    /// Reports that we are now connected to the given node.
    pub fn set_connected(self) -> EntryInKbucketConnPending<'a, TPeerId, TVal> {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryInKbucketDiscPending if we know of a bucket; QED");

        let mut pending = table.pending_node.as_mut()
            .expect("we can only build a EntryInKbucketDiscPending if there's a pending node; QED");
        debug_assert!(!pending.connected);
        pending.connected = true;

        EntryInKbucketConnPending {
            parent: self.parent,
            peer_id: self.peer_id,
        }
    }
}

/// Represents an entry in a k-bucket.
pub struct EntryInKbucketDisc<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a Key<TPeerId>,
}

impl<'a, TPeerId, TVal> EntryInKbucketDisc<'a, TPeerId, TVal>
where
    TPeerId: Clone,
{
    pub fn key(&self) -> &Key<TPeerId> {
        self.peer_id
    }

    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryInKbucketDisc if we know of a bucket; QED");

        let peer_id = self.peer_id;
        &mut table.nodes.iter_mut()
            .find(move |p| p.id == *peer_id)
            .expect("We can only build a EntryInKbucketDisc if we know that the peer is in its \
                     bucket; QED")
            .value
    }

    /// Sets the node as connected. This moves the entry in the bucket.
    pub fn set_connected(self) -> EntryInKbucketConn<'a, TPeerId, TVal> {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryInKbucketDisc if we know of a bucket; QED");

        let pos = {
            let peer_id = self.peer_id;
            table.nodes.iter().position(|p| p.id == *peer_id)
                .expect("We can only build a EntryInKbucketDisc if we know that the peer is in \
                         its bucket; QED")
        };

        // If the position marks the least-recently connected peer, it is now re-connected,
        // which means that the pending entry should be dropped, as preference is given to
        // previously known peers.
        //
        // Note that it is theoretically possible that the replacement could have occurred between
        // the moment when we build the `EntryInKbucketConn` and the moment when we call
        // `set_connected`, but we don't take that into account.
        if pos == 0 {
            table.pending_node = None;
        }

        // The entry for the most-recently connected peer goes at the end of the bucket.
        debug_assert!(table.first_connected_pos.map_or(true, |i| pos < i));
        let last = table.nodes.len() - 1;
        table.first_connected_pos = table.first_connected_pos.map(|p| p - 1).or(Some(last));
        let entry = table.nodes.remove(pos);
        table.nodes.push(entry);

        // There shouldn't be a pending node if all slots are full of connected nodes.
        debug_assert!(table.first_connected_pos != Some(0) || table.pending_node.is_none());

        EntryInKbucketConn {
            parent: self.parent,
            peer_id: self.peer_id,
        }
    }
}

/// Represents an entry not in any k-bucket.
pub struct EntryNotInKbucket<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a Key<TPeerId>,
}

impl<TPeerId, TVal> EntryNotInKbucket<'_, TPeerId, TVal>
where
    TPeerId: Clone,
{
    /// Inserts the node as connected, if possible.
    pub fn insert_connected(self, value: TVal) -> InsertOutcome<TPeerId> {
        let unresponsive_timeout = self.parent.unresponsive_timeout;

        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryNotInKbucket if we know of a bucket; QED");

        if table.nodes.is_full() {
            if table.first_connected_pos == Some(0) || table.pending_node.is_some() {
                InsertOutcome::Full
            } else {
                table.pending_node = Some(PendingNode {
                    node: Node { id: self.peer_id.clone(), value },
                    replace: Instant::now() + unresponsive_timeout,
                    connected: true,
                });
                InsertOutcome::Pending {
                    to_ping: table.nodes[0].id.clone()
                }
            }
        } else {
            // The most-recently connected peer is the last entry in the bucket.
            let pos = table.nodes.len();
            table.nodes.push(Node {
                id: self.peer_id.clone(),
                value,
            });
            table.first_connected_pos = table.first_connected_pos.or(Some(pos));
            InsertOutcome::Inserted
        }
    }

    /// Inserts the node as disconnected, if possible.
    ///
    /// > **Note**: This function will never return `Pending`. If the bucket is full, we simply
    /// >           do nothing.
    pub fn insert_disconnected(self, value: TVal) -> InsertOutcome<TPeerId> {
        let table = self.parent.bucket_mut(&self.peer_id)
                .expect("we can only build a EntryNotInKbucket if we know of a bucket; QED");

        if table.nodes.is_full() {
            InsertOutcome::Full
        } else {
            // The most-recently disconnected peer is the entry preceding the first
            // connected peer.
            let node = Node { id: self.peer_id.clone(), value };
            if let Some(ref mut first_connected_pos) = table.first_connected_pos {
                table.nodes.insert(*first_connected_pos, node);
                *first_connected_pos += 1;
            } else {
                table.nodes.push(node);
            }
            InsertOutcome::Inserted
        }
    }
}

/// The outcome of [`EntryNotInKbucket::insert_connected`] and
/// [`EntryNotInKbucket::insert_disconnected`].
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertOutcome<TPeerId> {
    /// The entry has been successfully inserted into a bucket.
    Inserted,
    /// The entry is pending insertion into a bucket that is currently full. The pending
    /// entry is inserted after a timeout elapsed, if the connection to the peer
    /// corresponding to the least-recently connected entry is not re-established within
    /// that timeout window.
    Pending {
        /// The identifier of the least-recently connected peer that is currently considered
        /// disconnected and should be checked for connectivity in order to prevent it from being
        /// replaced. If connectivity to the peer is re-established, the corresponding
        /// entry should be updated via [`EntryInKbucketDisc::set_connected`].
        to_ping: Key<TPeerId>,
    },
    /// The entry was not inserted because the bucket is filled with entries marked
    /// as connected and there already exists a pending entry for that bucket.
    Full,
}

