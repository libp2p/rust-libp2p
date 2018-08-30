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

use fnv::FnvHashMap;
use futures::{Async, Future, Poll, Stream, future, task};
use muxing;
use parking_lot::{Mutex, MutexGuard};
use std::collections::hash_map::Entry;
use std::fmt;
use std::io::Error as IoError;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use upgrade::{self, ConnectionUpgradeFilter};
use {ConnectionUpgrade, Endpoint, Multiaddr, PeerId, Transport};

/// Active connection to a node.
pub struct Node<TMuxer, TSubUpgrFut, TOutbound, THandlerOut> {
    /// Muxer used to open or receive new substreams.
    muxer: Arc<TMuxer>,

    /// If true, the muxer isn't capable of producing any more inbound substream.
    muxer_inbound_finished: bool,

    /// List of outbound substreams currently being opened.
    outbound_substreams: Vec<TOutbound>,

    /// Substreams whose full upgrade is currently being negotiated.
    /// Includes a list of tasks that must be notified when the upgrade completes.
    substream_upgrades: Vec<(TSubUpgrFut, FnvHashMap<usize, task::Task>)>,

    /// All the fully-negotiated substreams.
    full_substreams: Vec<THandlerOut>,
}

impl Node {
    /// Creates a new manager for this node.
    pub fn new(muxer: TMuxer) -> Self {
        Node {
            muxer: Arc::new(muxer),
            muxer_inbound_finished: false,
            outbound_substreams: Vec::with_capacity(4),
            substream_upgrades: Vec::with_capacity(4),
            full_substreams: Vec::with_capacity(16),
        }
    }

    /// Returns true if this node has any active substream.
    pub fn is_in_use(&self) -> bool {
        self.outbound_substreams.is_empty() && self.substream_upgrades.is_empty() &&
            self.full_substreams.is_empty()
    }
}

impl<'a, TTrans, TUpgrade, THandler, TMuxer, THandlerOut> Stream for
    &'a mut Node<TTrans, TUpgrade, THandler, TTrans::Listener, TTrans::ListenerUpgrade, upgrade::apply::UpgradeApplyFuture<muxing::SubstreamRef<Arc<TMuxer>>, TUpgrade, future::FutureResult<Multiaddr, IoError>>, TTrans::Dial, TMuxer, muxing::OutboundSubstreamRefWrapFuture<Arc<TMuxer>>, THandlerOut>
where
    TTrans: Transport<Output = (PeerId, TMuxer)> + Clone,
    TUpgrade: ConnectionUpgrade<muxing::SubstreamRef<Arc<TMuxer>>, future::FutureResult<Multiaddr, IoError>> + Clone,
    TUpgrade::NamesIter: Clone,
    THandler: FnMut(TUpgrade::Output, &PeerId) -> THandlerOut,
    TMuxer: muxing::StreamMuxer,
    THandlerOut: Stream<Error = IoError>,
{
    type Item = Event<THandlerOut, THandlerOut::Item>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Polling incoming substreams for that node.
        if !self.muxer_inbound_finished {
            match self.muxer.poll_inbound() {
                Ok(Async::Ready(Some(substream))) => {
                    let substream = muxing::substream_from_ref(self.muxer.clone(), substream);
                    let dummy_addr: Multiaddr = "/memory".parse().unwrap();     // TODO: addr
                    let upgrade = upgrade::apply(substream, self.upgrade.clone(), Endpoint::Listener, future::ok(dummy_addr));  // TODO: addr
                    self.substream_upgrades.push((upgrade, FnvHashMap::with_capacity_and_hasher(2, Default::default())));
                },
                Ok(Async::NotReady) => {},
                Ok(Async::Ready(None)) => {
                    self.muxer_inbound_finished = true;
                    return Ok(Async::Ready(Some(SwarmEvent::MuxerInboundFinished)));
                },
                Err(err) => {
                    self.muxer_inbound_finished = true;
                    return Ok(Async::Ready(Some(SwarmEvent::MuxerInboundError(err))));
                },
            }
        }

        // Polling substreams being opened.
        // We remove each element one by one and add them back only if relevant.
        for n in (0 .. self.outbound_substreams.len()).rev() {
            let mut substream = self.outbound_substreams.swap_remove(n);
            match substream.poll() {
                Ok(Async::Ready(Some(substream))) => {
                    let dummy_addr: Multiaddr = "/memory".parse().unwrap();     // TODO: addr
                    let upgrade = upgrade::apply(substream, self.upgrade.clone(), Endpoint::Dialer, future::ok(dummy_addr));  // TODO: addr
                    self.substream_upgrades.push((upgrade, FnvHashMap::with_capacity_and_hasher(2, Default::default())));
                },
                Ok(Async::Ready(None)) => {
                    // TODO: notify some tasks so that we wake up the dialing attempts
                },
                Err(error) => {
                    debug!("Error in processing: {:?}", error);
                    // TODO: notify some tasks so that we wake up the dialing attempts
                    /*return Ok(Async::Ready(Some(SwarmEvent::SubstreamError {
                        peer_id: peer_id.clone(),
                        substream,
                        error,
                    })));*/
                },
                Ok(Async::NotReady) => {
                    self.outbound_substreams.push(substream);
                },
            }
        }

        // Polling substreams that are being negotiated.
        // We remove each element one by one and add them back only if relevant.
        for n in (0 .. self.substream_upgrades.len()).rev() {
            let (mut upgrade, tasks) = self.substream_upgrades.swap_remove(n);
            match upgrade.poll() {
                Ok(Async::NotReady) => {
                    self.substream_upgrades.push((upgrade, tasks));
                },
                Ok(Async::Ready((output, _multiaddr_future))) => {
                    trace!("Successfully negotiated substream with {:?}", peer_id);
                    // TODO: unlock mutex before calling handler, in order to avoid deadlocks if
                    // the user does something stupid
                    self.full_substreams.push(handler(output, peer_id));
                    for (_, task) in tasks {
                        task.notify();
                    }
                },
                Err(error) => {
                    for (_, task) in tasks {
                        task.notify();
                    }
                    trace!("Error while upgrading substream with {:?}: {:?}", peer_id, error);
                    return Ok(Async::Ready(Some(SwarmEvent::SubstreamUpgradeError(error))));
                },
            }
        }

        // Polling fully-negotiated substreams.
        // We remove each element one by one and add them back only if relevant.
        for n in (0 .. self.full_substreams.len()).rev() {
            let mut substream = self.full_substreams.swap_remove(n);
            match substream.poll() {
                Ok(Async::NotReady) => {
                    self.full_substreams.push(substream);
                },
                Ok(Async::Ready(Some(event))) => {
                    return Ok(Async::Ready(Some(SwarmEvent::FullSubstreamEvent(event))));
                },
                Ok(Async::Ready(None)) => {
                    trace!("Future returned by swarm handler driven to completion");
                    return Ok(Async::Ready(Some(SwarmEvent::FullSubstreamClosed(substream))));
                },
                Err(error) => {
                    debug!("Error in processing: {:?}", error);
                    return Ok(Async::Ready(Some(SwarmEvent::FullSubstreamError {
                        substream,
                        error,
                    })));
                },
            }
        }

        // TODO: we never return `Ok(Ready)` because there's no way to know whether
        //       `next_incoming()` can produce anything more in the future ; also we would need to
        //       know when the controller has been dropped
        shared.task_to_notify = Some(task::current());
        Ok(Async::NotReady)
    }
}

/// Event that can happen during the processing of the swarm.
#[derive(Debug)]
pub enum Event<THandlerOut, TEvent> {
    /// The muxer cannot produce any more incoming substream.
    MuxerInboundFinished,

    /// Error while polling a muxer for incoming substreams.
    MuxerInboundError(IoError),

    /// An error happened while upgrading a substream.
    SubstreamUpgradeError(IoError),

    /// A fully negotiated substream has produced an event.
    FullSubstreamEvent(TEvent),

    /// A fully negotiated substream has closed gracefully.
    ///
    /// This happens when the future is finished. Contains the future.
    FullSubstreamClosed(THandlerOut),

    /// A fully-negotiated substream has errored. Contains the future and the error.
    FullSubstreamError {
        substream: THandlerOut,
        error: IoError,
    },
}
