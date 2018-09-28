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
use futures::prelude::*;
use muxing::StreamMuxer;
use nodes::node::Substream;
use nodes::handled_node_tasks::{HandledNodesEvent, HandledNodesTasks};
use nodes::handled_node_tasks::{Task as HandledNodesTask, TaskId};
use nodes::handled_node::NodeHandler;
use std::collections::hash_map::Entry;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use void::Void;
use {Multiaddr, PeerId};

// TODO: make generic over PeerId

/// Implementation of `Stream` that handles a collection of nodes.
// TODO: implement Debug
pub struct CollectionStream<TInEvent, TOutEvent> {
    /// Object that handles the tasks.
    inner: HandledNodesTasks<TInEvent, TOutEvent>,
    /// List of nodes, with the task id that handles this node. The corresponding entry in `tasks`
    /// must always be in the `Connected` state.
    nodes: FnvHashMap<PeerId, TaskId>,
    /// List of tasks and their state. If `Connected`, then a corresponding entry must be present
    /// in `nodes`.
    tasks: FnvHashMap<TaskId, TaskState>,
}

/// State of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState {
    /// Task is attempting to reach a peer.
    Pending,
    /// The task is connected to a peer.
    Connected(PeerId),
}

/// Event that can happen on the `CollectionStream`.
// TODO: implement Debug
pub enum CollectionEvent<TOutEvent> {
    /// A connection to a node has succeeded.
    NodeReached {
        /// Identifier of the node.
        peer_id: PeerId,
        /// Identifier of the reach attempt that succeeded.
        id: ReachAttemptId,
    },

    /// A connection to a node has succeeded and replaces a former connection.
    ///
    /// The opened substreams of the former node will keep working (unless the remote decides to
    /// close them).
    NodeReplaced {
        /// Identifier of the node.
        peer_id: PeerId,
        /// Identifier of the reach attempt that succeeded.
        id: ReachAttemptId,
    },

    /// A connection to a node has been closed.
    ///
    /// This happens once both the inbound and outbound channels are closed, and no more outbound
    /// substream attempt is pending.
    NodeClosed {
        /// Identifier of the node.
        peer_id: PeerId,
    },

    /// A connection to a node has errored.
    NodeError {
        /// Identifier of the node.
        peer_id: PeerId,
        /// The error that happened.
        error: IoError,
    },

    /// An error happened on the future that was trying to reach a node.
    ReachError {
        /// Identifier of the reach attempt that failed.
        id: ReachAttemptId,
        /// Error that happened on the future.
        error: IoError,
    },

    /// A node has produced an event.
    NodeEvent {
        /// Identifier of the node.
        peer_id: PeerId,
        /// The produced event.
        event: TOutEvent,
    },
}

/// Identifier for a future that attempts to reach a node.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReachAttemptId(TaskId);

impl<TInEvent, TOutEvent> CollectionStream<TInEvent, TOutEvent> {
    /// Creates a new empty collection.
    #[inline]
    pub fn new() -> Self {
        CollectionStream {
            inner: HandledNodesTasks::new(),
            nodes: Default::default(),
            tasks: Default::default(),
        }
    }

    /// Adds to the collection a future that tries to reach a remote.
    ///
    /// This method spawns a task dedicated to resolving this future and processing the node's
    /// events.
    pub fn add_reach_attempt<TFut, TMuxer, TAddrFut, THandler>(&mut self, future: TFut, handler: THandler)
        -> ReachAttemptId
    where
        TFut: Future<Item = ((PeerId, TMuxer), TAddrFut), Error = IoError> + Send + 'static,
        TAddrFut: Future<Item = Multiaddr, Error = IoError> + Send + 'static,
        THandler: NodeHandler<Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        THandler::OutboundOpenInfo: Send + 'static,     // TODO: shouldn't be required?
        TMuxer: StreamMuxer + Send + Sync + 'static,  // TODO: Send + Sync + 'static shouldn't be required
        TMuxer::OutboundSubstream: Send + 'static,  // TODO: shouldn't be required
    {
        let id = self.inner.add_reach_attempt(future, handler);
        self.tasks.insert(id, TaskState::Pending);
        ReachAttemptId(id)
    }

    /// Interrupts a reach attempt.
    ///
    /// Returns `Ok` if something was interrupted, and `Err` if the ID is not or no longer valid.
    pub fn interrupt(&mut self, id: ReachAttemptId) -> Result<(), ()> {
        match self.tasks.entry(id.0) {
            Entry::Vacant(_) => Err(()),
            Entry::Occupied(entry) => {
                match entry.get() {
                    &TaskState::Connected(_) => return Err(()),
                    &TaskState::Pending => (),
                };

                entry.remove();
                self.inner.task(id.0)
                    .expect("whenever we receive a TaskClosed event or interrupt a task, we \
                             remove the corresponding entry from self.tasks ; therefore all \
                             elements in self.tasks are valid tasks in the \
                             HandledNodesTasks ; qed")
                    .close();

                Ok(())
            }
        }
    }

    /// Sends an event to all nodes.
    #[inline]
    pub fn broadcast_event(&mut self, event: &TInEvent)
    where TInEvent: Clone,
    {
        // TODO: remove the ones we're not connected to?
        self.inner.broadcast_event(event)
    }

    /// Grants access to an object that allows controlling a peer of the collection.
    ///
    /// Returns `None` if we don't have a connection to this peer.
    #[inline]
    pub fn peer_mut(&mut self, id: &PeerId) -> Option<PeerMut<TInEvent>> {
        let task = match self.nodes.get(id) {
            Some(&task) => task,
            None => return None,
        };

        match self.inner.task(task) {
            Some(inner) => Some(PeerMut {
                inner,
                tasks: &mut self.tasks,
                nodes: &mut self.nodes,
            }),
            None => None,
        }
    }

    /// Returns true if we are connected to the given peer.
    ///
    /// This will return true only after a `NodeReached` event has been produced by `poll()`.
    #[inline]
    pub fn has_connection(&self, id: &PeerId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Returns a list of all the active connections.
    ///
    /// Does not include reach attempts that haven't reached any target yet.
    #[inline]
    pub fn connections(&self) -> impl Iterator<Item = &PeerId> {
        self.nodes.keys()
    }
}

/// Access to a peer in the collection.
pub struct PeerMut<'a, TInEvent: 'a> {
    inner: HandledNodesTask<'a, TInEvent>,
    tasks: &'a mut FnvHashMap<TaskId, TaskState>,
    nodes: &'a mut FnvHashMap<PeerId, TaskId>,
}

impl<'a, TInEvent> PeerMut<'a, TInEvent> {
    /// Sends an event to the given node.
    #[inline]
    pub fn send_event(&mut self, event: TInEvent) {
        self.inner.send_event(event)
    }

    /// Closes the connections to this node.
    ///
    /// No further event will be generated for this node.
    pub fn close(self) {
        let task_state = self.tasks.remove(&self.inner.id());
        if let Some(TaskState::Connected(peer_id)) = task_state {
            let old_task_id = self.nodes.remove(&peer_id);
            debug_assert_eq!(old_task_id, Some(self.inner.id()));
        } else {
            panic!("a PeerMut can only be created if an entry is present in nodes ; an entry in \
                    nodes always matched a Connected entry in tasks ; qed");
        };

        self.inner.close();
    }
}

impl<TInEvent, TOutEvent> Stream for CollectionStream<TInEvent, TOutEvent> {
    type Item = CollectionEvent<TOutEvent>;
    type Error = Void; // TODO: use ! once stable

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let item = try_ready!(self.inner.poll());

        match item {
            Some(HandledNodesEvent::TaskClosed { id, result }) => {
                match (self.tasks.remove(&id), result) {
                    (Some(TaskState::Pending), Err(err)) => {
                        Ok(Async::Ready(Some(CollectionEvent::ReachError {
                            id: ReachAttemptId(id),
                            error: err,
                        })))
                    },
                    (Some(TaskState::Pending), Ok(())) => {
                        // TODO: this variant shouldn't happen ; prove this
                        Ok(Async::Ready(Some(CollectionEvent::ReachError {
                            id: ReachAttemptId(id),
                            error: IoError::new(IoErrorKind::Other, "couldn't reach the node"),
                        })))
                    },
                    (Some(TaskState::Connected(peer_id)), Ok(())) => {
                        let _node_task_id = self.nodes.remove(&peer_id);
                        debug_assert_eq!(_node_task_id, Some(id));
                        Ok(Async::Ready(Some(CollectionEvent::NodeClosed {
                            peer_id,
                        })))
                    },
                    (Some(TaskState::Connected(peer_id)), Err(err)) => {
                        let _node_task_id = self.nodes.remove(&peer_id);
                        debug_assert_eq!(_node_task_id, Some(id));
                        Ok(Async::Ready(Some(CollectionEvent::NodeError {
                            peer_id,
                            error: err,
                        })))
                    },
                    (None, _) => {
                        panic!("self.tasks is always kept in sync with the tasks in self.inner ; \
                                when we add a task in self.inner we add a corresponding entry in \
                                self.tasks, and remove the entry only when the task is closed ; \
                                qed")
                    },
                }
            },
            Some(HandledNodesEvent::NodeReached { id, peer_id }) => {
                // Set the state of the task to `Connected`.
                let former_task_id = self.nodes.insert(peer_id.clone(), id);
                let _former_state = self.tasks.insert(id, TaskState::Connected(peer_id.clone()));
                debug_assert_eq!(_former_state, Some(TaskState::Pending));

                // It is possible that we already have a task connected to the same peer. In this
                // case, we need to emit a `NodeReplaced` event.
                if let Some(former_task_id) = former_task_id {
                    self.inner.task(former_task_id)
                        .expect("whenever we receive a TaskClosed event or close a node, we \
                                 remove the corresponding entry from self.nodes ; therefore all \
                                 elements in self.nodes are valid tasks in the \
                                 HandledNodesTasks ; qed")
                        .close();
                    let _former_other_state = self.tasks.remove(&former_task_id);
                    debug_assert_eq!(_former_other_state, Some(TaskState::Connected(peer_id.clone())));

                    Ok(Async::Ready(Some(CollectionEvent::NodeReplaced {
                        peer_id,
                        id: ReachAttemptId(id),
                    })))

                } else {
                    Ok(Async::Ready(Some(CollectionEvent::NodeReached {
                        peer_id,
                        id: ReachAttemptId(id),
                    })))
                }
            },
            Some(HandledNodesEvent::NodeEvent { id, event }) => {
                let peer_id = match self.tasks.get(&id) {
                    Some(TaskState::Connected(peer_id)) => peer_id.clone(),
                    _ => panic!("we can only receive NodeEvent events from a task after we \
                                 received a corresponding NodeReached event from that same task ; \
                                 when we receive a NodeReached event, we ensure that the entry in \
                                 self.tasks is switched to the Connected state ; qed"),
                };

                Ok(Async::Ready(Some(CollectionEvent::NodeEvent {
                    peer_id,
                    event,
                })))
            }
            None => Ok(Async::Ready(None)),
        }
    }
}
