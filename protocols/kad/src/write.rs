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

//! Contains the state of the second stage of PUT_VALUE process of Kademlia.

use fnv::FnvHashMap;

/// The state of the single peer.
#[derive(Clone)]
enum PeerState {
    /// We don't know yet.
    Unknown,

    /// Putting a value failed.
    Failed,

    /// Putting a value succeeded.
    Succeeded,
}

/// State of the `PUV_VALUE` second stage
///
/// Here we are gathering the results of all `PUT_VALUE` requests that we've
/// sent to the appropriate peers. We keep track of the set of peers that we've
/// sent the requests to and the counts for error and normal responses
pub struct WriteState<TPeerId, TTarget> {
    /// The key that we're inserting into the dht.
    target: TTarget,

    /// The peers thae we'are asking to store our value.
    peers: FnvHashMap<TPeerId, PeerState>,

    /// The count of successful stores.
    successes: usize,

    /// The count of errors.
    failures: usize,
}

impl<TPeerId, TTarget> WriteState<TPeerId, TTarget>
where
    TPeerId: std::hash::Hash + Clone + Eq
{
    /// Creates a new WriteState.
    ///
    /// Stores the state of an ongoing second stage of a PUT_VALUE process
    pub fn new(target: TTarget, peers: Vec<TPeerId>) -> Self {
        use std::iter::FromIterator;
        WriteState {
            target,
            peers: FnvHashMap::from_iter(peers
                                         .into_iter()
                                         .zip(std::iter::repeat(PeerState::Unknown))
            ),
            successes: 0,
            failures: 0,
        }
    }

    /// Inform the state that writing to one of the target peers has succeeded
    pub fn inject_write_success(&mut self, peer: &TPeerId) {
        if let Some(state @ PeerState::Unknown) = self.peers.get_mut(peer) {
            *state = PeerState::Succeeded;
            self.successes += 1;
        }
    }

    /// Inform the state that writing to one of the target peers has failed
    pub fn inject_write_error(&mut self, peer: &TPeerId) {
        if let Some(state @ PeerState::Unknown) = self.peers.get_mut(peer) {
            *state = PeerState::Failed;
            self.failures += 1;
        }
    }

    /// Ask the state if it is done
    // TODO: probably it should also be a poll() in the fashion of QueryState and have a timeout
    pub fn done(&self) -> bool {
        self.peers.len() == self.successes + self.failures
    }

    /// Consume the state and return a list of target peers and succeess/error counters
    pub fn into_inner(self) -> (TTarget, usize, usize) {
        (self.target, self.successes, self.failures)
    }
}
