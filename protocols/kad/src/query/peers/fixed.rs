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

use super::*;

use fnv::FnvHashMap;
use libp2p_core::PeerId;
use std::{vec, collections::hash_map::Entry};

/// A peer iterator for a fixed set of peers.
pub struct FixedPeersIter {
    /// Ther permitted parallelism, i.e. number of pending results.
    parallelism: usize,

    /// The state of peers emitted by the iterator.
    peers: FnvHashMap<PeerId, PeerState>,

    /// The backlog of peers that can still be emitted.
    iter: vec::IntoIter<PeerId>,

    /// The internal state of the iterator.
    state: State,
}

enum State {
    Waiting { num_waiting: usize },
    Finished
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum PeerState {
    /// The iterator is waiting for a result to be reported back for the peer.
    Waiting,

    /// The iterator has been informed that the attempt to contact the peer failed.
    Failed,

    /// The iterator has been informed of a successful result from the peer.
    Succeeded,
}

impl FixedPeersIter {
    pub fn new(peers: Vec<PeerId>, parallelism: usize) -> Self {
        Self {
            parallelism,
            peers: FnvHashMap::default(),
            iter: peers.into_iter(),
            state: State::Waiting { num_waiting: 0 },
        }
    }

    pub fn on_success(&mut self, peer: &PeerId) {
        if let State::Waiting { num_waiting } = &mut self.state {
            if let Some(state @ PeerState::Waiting) = self.peers.get_mut(peer) {
                *state = PeerState::Succeeded;
                *num_waiting -= 1;
            }
        }
    }

    pub fn on_failure(&mut self, peer: &PeerId) {
        if let State::Waiting { .. } = &self.state {
            if let Some(state @ PeerState::Waiting) = self.peers.get_mut(peer) {
                *state = PeerState::Failed;
            }
        }
    }

    pub fn is_waiting(&self, peer: &PeerId) -> bool {
        self.peers.get(peer) == Some(&PeerState::Waiting)
    }

    pub fn finish(&mut self) {
        if let State::Waiting { .. } = self.state {
            self.state = State::Finished
        }
    }

    pub fn next(&mut self) -> PeersIterState {
        match &mut self.state {
            State::Finished => return PeersIterState::Finished,
            State::Waiting { num_waiting } => {
                if *num_waiting >= self.parallelism {
                    return PeersIterState::WaitingAtCapacity
                }
                loop {
                    match self.iter.next() {
                        None => if *num_waiting == 0 {
                            self.state = State::Finished;
                            return PeersIterState::Finished
                        } else {
                            return PeersIterState::Waiting(None)
                        }
                        Some(p) => match self.peers.entry(p.clone()) {
                            Entry::Occupied(_) => {} // skip duplicates
                            Entry::Vacant(e) => {
                                *num_waiting += 1;
                                e.insert(PeerState::Waiting);
                                return PeersIterState::Waiting(Some(Cow::Owned(p)))
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn into_result(self) -> impl Iterator<Item = PeerId> {
        self.peers.into_iter()
            .filter_map(|(p, s)|
                if let PeerState::Succeeded = s {
                    Some(p)
                } else {
                    None
                })
    }
}

