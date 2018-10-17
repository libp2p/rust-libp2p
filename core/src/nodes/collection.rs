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
use std::{collections::hash_map::Entry, fmt, io, mem};
use PeerId;

// TODO: make generic over PeerId

/// Implementation of `Stream` that handles a collection of nodes.
pub struct CollectionStream<TInEvent, TOutEvent, THandler> {
    /// Object that handles the tasks.
    inner: HandledNodesTasks<TInEvent, TOutEvent, THandler>,
    /// List of nodes, with the task id that handles this node. The corresponding entry in `tasks`
    /// must always be in the `Connected` state.
    nodes: FnvHashMap<PeerId, TaskId>,
    /// List of tasks and their state. If `Connected`, then a corresponding entry must be present
    /// in `nodes`.
    tasks: FnvHashMap<TaskId, TaskState>,
}

impl<TInEvent, TOutEvent, THandler> fmt::Debug for CollectionStream<TInEvent, TOutEvent, THandler> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let mut list = f.debug_list();
        for (id, task) in &self.tasks {
            match *task {
                TaskState::Pending => {
                    list.entry(&format!("Pending({:?})", ReachAttemptId(*id)))
                },
                TaskState::Connected(ref peer_id) => {
                    list.entry(&format!("Connected({:?})", peer_id))
                }
            };
        }
        list.finish()
    }
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
pub enum CollectionEvent<'a, TInEvent:'a , TOutEvent: 'a, THandler: 'a> {
    /// A connection to a node has succeeded. You must use the provided event in order to accept
    /// the connection.
    NodeReached(CollectionReachEvent<'a, TInEvent, TOutEvent, THandler>),

    /// A connection to a node has been closed.
    ///
    /// This happens once both the inbound and outbound channels are closed, and no more outbound
    /// substream attempt is pending.
    NodeClosed {
        /// Identifier of the node.
        peer_id: PeerId,
    },

    /// A connection to a node has errored.
    ///
    /// Can only happen after a node has been successfully reached.
    NodeError {
        /// Identifier of the node.
        peer_id: PeerId,
        /// The error that happened.
        error: io::Error,
    },

    /// An error happened on the future that was trying to reach a node.
    ReachError {
        /// Identifier of the reach attempt that failed.
        id: ReachAttemptId,
        /// Error that happened on the future.
        error: io::Error,
        /// The handler that was passed to `add_reach_attempt`.
        handler: THandler,
    },

    /// A node has produced an event.
    NodeEvent {
        /// Identifier of the node.
        peer_id: PeerId,
        /// The produced event.
        event: TOutEvent,
    },
}

impl<'a, TInEvent, TOutEvent, THandler> fmt::Debug for CollectionEvent<'a, TInEvent, TOutEvent, THandler>
where TOutEvent: fmt::Debug
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            CollectionEvent::NodeReached(ref inner) => {
                f.debug_tuple("CollectionEvent::NodeReached")
                .field(inner)
                .finish()
            },
            CollectionEvent::NodeClosed { ref peer_id } => {
                f.debug_struct("CollectionEvent::NodeClosed")
                .field("peer_id", peer_id)
                .finish()
            },
            CollectionEvent::NodeError { ref peer_id, ref error } => {
                f.debug_struct("CollectionEvent::NodeError")
                .field("peer_id", peer_id)
                .field("error", error)
                .finish()
            },
            CollectionEvent::ReachError { ref id, ref error, .. } => {
                f.debug_struct("CollectionEvent::ReachError")
                .field("id", id)
                .field("error", error)
                .finish()
            },
            CollectionEvent::NodeEvent { ref peer_id, ref event } => {
                f.debug_struct("CollectionEvent::NodeEvent")
                .field("peer_id", peer_id)
                .field("event", event)
                .finish()
            },
        }
    }
}

/// Event that happens when we reach a node.
#[must_use = "The node reached event is used to accept the newly-opened connection"]
pub struct CollectionReachEvent<'a, TInEvent: 'a, TOutEvent: 'a, THandler: 'a> {
    /// Peer id we connected to.
    peer_id: PeerId,
    /// The task id that reached the node.
    id: TaskId,
    /// The `CollectionStream` we are referencing.
    parent: &'a mut CollectionStream<TInEvent, TOutEvent, THandler>,
}

impl<'a, TInEvent, TOutEvent, THandler> CollectionReachEvent<'a, TInEvent, TOutEvent, THandler> {
    /// Returns the peer id the node that has been reached.
    #[inline]
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the reach attempt that reached the node.
    #[inline]
    pub fn reach_attempt_id(&self) -> ReachAttemptId {
        ReachAttemptId(self.id)
    }

    /// Returns `true` if accepting this reached node would replace an existing connection to that
    /// node.
    #[inline]
    pub fn would_replace(&self) -> bool {
        self.parent.nodes.contains_key(&self.peer_id)
    }

    /// Accepts the new node.
    pub fn accept(self) -> (CollectionNodeAccept, PeerId) {
        // Set the state of the task to `Connected`.
        let former_task_id = self.parent.nodes.insert(self.peer_id.clone(), self.id);
        let _former_state = self.parent.tasks.insert(self.id, TaskState::Connected(self.peer_id.clone()));
        debug_assert_eq!(_former_state, Some(TaskState::Pending));

        // It is possible that we already have a task connected to the same peer. In this
        // case, we need to emit a `NodeReplaced` event.
        let ret_value = if let Some(former_task_id) = former_task_id {
            self.parent.inner.task(former_task_id)
                .expect("whenever we receive a TaskClosed event or close a node, we remove the \
                         corresponding entry from self.nodes ; therefore all elements in \
                         self.nodes are valid tasks in the HandledNodesTasks ; qed")
                .close();
            let _former_other_state = self.parent.tasks.remove(&former_task_id);
            debug_assert_eq!(_former_other_state, Some(TaskState::Connected(self.peer_id.clone())));

            // TODO: we unfortunately have to clone the peer id here
            (CollectionNodeAccept::ReplacedExisting, self.peer_id.clone())
        } else {
            // TODO: we unfortunately have to clone the peer id here
            (CollectionNodeAccept::NewEntry, self.peer_id.clone())
        };

        // Don't run the destructor.
        mem::forget(self);

        ret_value
    }

    /// Denies the node.
    ///
    /// Has the same effect as dropping the event without accepting it.
    #[inline]
    pub fn deny(self) -> PeerId {
        // TODO: we unfortunately have to clone the id here, in order to be explicit
        let peer_id = self.peer_id.clone();
        drop(self);  // Just to be explicit
        peer_id
    }
}

impl<'a, TInEvent, TOutEvent, THandler> fmt::Debug for CollectionReachEvent<'a, TInEvent, TOutEvent, THandler> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("CollectionReachEvent")
            .field("peer_id", &self.peer_id)
            .field("reach_attempt_id", &self.reach_attempt_id())
            .finish()
    }
}

impl<'a, TInEvent, TOutEvent, THandler> Drop for CollectionReachEvent<'a, TInEvent, TOutEvent, THandler> {
    fn drop(&mut self) {
        let task_state = self.parent.tasks.remove(&self.id);
        debug_assert!(if let Some(TaskState::Pending) = task_state { true } else { false });
        self.parent.inner.task(self.id)
            .expect("we create the CollectionReachEvent with a valid task id ; the \
                     CollectionReachEvent mutably borrows the collection, therefore nothing \
                     can delete this task during the lifetime of the CollectionReachEvent ; \
                     therefore the task is still valid when we delete it ; qed")
            .close();
    }
}

/// Outcome of accepting a node.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CollectionNodeAccept {
    /// We replaced an existing node.
    ReplacedExisting,
    /// We didn't replace anything existing.
    NewEntry,
}

/// Identifier for a future that attempts to reach a node.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReachAttemptId(TaskId);

impl<TInEvent, TOutEvent, THandler> CollectionStream<TInEvent, TOutEvent, THandler> {
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
    pub fn add_reach_attempt<TFut, TMuxer>(&mut self, future: TFut, handler: THandler)
        -> ReachAttemptId
    where
        TFut: Future<Item = (PeerId, TMuxer), Error = io::Error> + Send + 'static,
        THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
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

    /// Provides an API similar to `Stream`, except that it cannot error.
    ///
    /// > **Note**: we use a regular `poll` method instead of implementing `Stream` in order to
    /// > remove the `Err` variant, but also because we want the `CollectionStream` to stay
    /// > borrowed if necessary.
    pub fn poll(&mut self) -> Async<CollectionEvent<TInEvent, TOutEvent, THandler>> {
        let item = match self.inner.poll() {
            Async::Ready(item) => item,
            Async::NotReady => return Async::NotReady,
        };

        match item {
            HandledNodesEvent::TaskClosed { id, result, handler } => {
                match (self.tasks.remove(&id), result, handler) {
                    (Some(TaskState::Pending), Err(err), Some(handler)) => {
                        Async::Ready(CollectionEvent::ReachError {
                            id: ReachAttemptId(id),
                            error: err,
                            handler,
                        })
                    },
                    (Some(TaskState::Pending), _, _) => {
                        // TODO: this variant shouldn't happen ; prove this
                        panic!()
                    },
                    (Some(TaskState::Connected(peer_id)), Ok(()), _handler) => {
                        debug_assert!(_handler.is_none());
                        let _node_task_id = self.nodes.remove(&peer_id);
                        debug_assert_eq!(_node_task_id, Some(id));
                        Async::Ready(CollectionEvent::NodeClosed {
                            peer_id,
                        })
                    },
                    (Some(TaskState::Connected(peer_id)), Err(err), _handler) => {
                        debug_assert!(_handler.is_none());
                        let _node_task_id = self.nodes.remove(&peer_id);
                        debug_assert_eq!(_node_task_id, Some(id));
                        Async::Ready(CollectionEvent::NodeError {
                            peer_id,
                            error: err,
                        })
                    },
                    (None, _, _) => {
                        panic!("self.tasks is always kept in sync with the tasks in self.inner ; \
                                when we add a task in self.inner we add a corresponding entry in \
                                self.tasks, and remove the entry only when the task is closed ; \
                                qed")
                    },
                }
            },
            HandledNodesEvent::NodeReached { id, peer_id } => {
                Async::Ready(CollectionEvent::NodeReached(CollectionReachEvent {
                    parent: self,
                    id,
                    peer_id,
                }))
            },
            HandledNodesEvent::NodeEvent { id, event } => {
                let peer_id = match self.tasks.get(&id) {
                    Some(TaskState::Connected(peer_id)) => peer_id.clone(),
                    _ => panic!("we can only receive NodeEvent events from a task after we \
                                 received a corresponding NodeReached event from that same task ; \
                                 when we receive a NodeReached event, we ensure that the entry in \
                                 self.tasks is switched to the Connected state ; qed"),
                };

                Async::Ready(CollectionEvent::NodeEvent {
                    peer_id,
                    event,
                })
            }
        }
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
