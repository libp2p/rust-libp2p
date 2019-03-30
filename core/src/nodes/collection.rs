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

use crate::{
    PeerId,
    muxing::StreamMuxer,
    nodes::{
        node::Substream,
        handled_node_tasks::{HandledNodesEvent, HandledNodesTasks, TaskClosedEvent},
        handled_node_tasks::{IntoNodeHandler, Task as HandledNodesTask, TaskId, ClosedTask},
        handled_node::{HandledNodeError, NodeHandler}
    }
};
use fnv::FnvHashMap;
use futures::prelude::*;
use std::{error, fmt, hash::Hash, mem};

mod tests;

/// Implementation of `Stream` that handles a collection of nodes.
pub struct CollectionStream<TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId = PeerId> {
    /// Object that handles the tasks.
    ///
    /// The user data contains the state of the task. If `Connected`, then a corresponding entry
    /// must be present in `nodes`.
    inner: HandledNodesTasks<TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TaskState<TPeerId, TUserData>, TPeerId>,

    /// List of nodes, with the task id that handles this node. The corresponding entry in `tasks`
    /// must always be in the `Connected` state.
    nodes: FnvHashMap<TPeerId, TaskId>,
}

impl<TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId> fmt::Debug for
    CollectionStream<TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
where
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_tuple("CollectionStream").finish()
    }
}

/// State of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState<TPeerId, TUserData> {
    /// Task is attempting to reach a peer.
    Pending,
    /// The task is connected to a peer.
    Connected(TPeerId, TUserData),
}

/// Event that can happen on the `CollectionStream`.
pub enum CollectionEvent<'a, TInEvent:'a , TOutEvent: 'a, THandler: 'a, TReachErr, THandlerErr, TUserData, TPeerId> {
    /// A connection to a node has succeeded. You must use the provided event in order to accept
    /// the connection.
    NodeReached(CollectionReachEvent<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>),

    /// A connection to a node has errored.
    ///
    /// Can only happen after a node has been successfully reached.
    NodeClosed {
        /// Identifier of the node.
        peer_id: TPeerId,
        /// The error that happened.
        error: HandledNodeError<THandlerErr>,
        /// User data that was passed when accepting.
        user_data: TUserData,
    },

    /// An error happened on the future that was trying to reach a node.
    ReachError {
        /// Identifier of the reach attempt that failed.
        id: ReachAttemptId,
        /// Error that happened on the future.
        error: TReachErr,
        /// The handler that was passed to `add_reach_attempt`.
        handler: THandler,
    },

    /// A node has produced an event.
    NodeEvent {
        /// The node that has generated the event.
        peer: PeerMut<'a, TInEvent, TUserData, TPeerId>,
        /// The produced event.
        event: TOutEvent,
    },
}

impl<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId> fmt::Debug for
    CollectionEvent<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
where TOutEvent: fmt::Debug,
      TReachErr: fmt::Debug,
      THandlerErr: fmt::Debug,
      TPeerId: Eq + Hash + Clone + fmt::Debug,
      TUserData: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            CollectionEvent::NodeReached(ref inner) => {
                f.debug_tuple("CollectionEvent::NodeReached")
                .field(inner)
                .finish()
            },
            CollectionEvent::NodeClosed { ref peer_id, ref error, ref user_data } => {
                f.debug_struct("CollectionEvent::NodeClosed")
                .field("peer_id", peer_id)
                .field("user_data", user_data)
                .field("error", error)
                .finish()
            },
            CollectionEvent::ReachError { ref id, ref error, .. } => {
                f.debug_struct("CollectionEvent::ReachError")
                .field("id", id)
                .field("error", error)
                .finish()
            },
            CollectionEvent::NodeEvent { ref peer, ref event } => {
                f.debug_struct("CollectionEvent::NodeEvent")
                .field("peer_id", peer.id())
                .field("event", event)
                .finish()
            },
        }
    }
}

/// Event that happens when we reach a node.
#[must_use = "The node reached event is used to accept the newly-opened connection"]
pub struct CollectionReachEvent<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId = PeerId> {
    /// Peer id we connected to.
    peer_id: TPeerId,
    /// The task id that reached the node.
    id: TaskId,
    /// The `CollectionStream` we are referencing.
    parent: &'a mut CollectionStream<TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>,
}

impl<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
    CollectionReachEvent<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
where
    TPeerId: Eq + Hash + Clone,
{
    /// Returns the peer id of the node that has been reached.
    #[inline]
    pub fn peer_id(&self) -> &TPeerId {
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
    pub fn accept(self, user_data: TUserData) -> (CollectionNodeAccept<TUserData>, TPeerId) {
        // Set the state of the task to `Connected`.
        let former_task_id = self.parent.nodes.insert(self.peer_id.clone(), self.id);
        *self.parent.inner.task(self.id)
            .expect("A CollectionReachEvent is only ever created from a valid attempt; QED")
            .user_data_mut() = TaskState::Connected(self.peer_id.clone(), user_data);

        // It is possible that we already have a task connected to the same peer. In this
        // case, we need to emit a `NodeReplaced` event.
        let tasks = &mut self.parent.inner;
        let ret_value = if let Some(former_task) = former_task_id.and_then(|i| tasks.task(i)) {
            debug_assert!(match *former_task.user_data() {
                TaskState::Connected(ref p, _) if *p == self.peer_id => true,
                _ => false
            });
            let user_data = match former_task.close().into_user_data() {
                TaskState::Connected(_, user_data) => user_data,
                _ => panic!("The former task was picked from `nodes`; all the nodes in `nodes` \
                             are always in the connected state")
            };
            // TODO: we unfortunately have to clone the peer id here
            (CollectionNodeAccept::ReplacedExisting(user_data), self.peer_id.clone())
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
    pub fn deny(self) -> TPeerId {
        // TODO: we unfortunately have to clone the id here, in order to be explicit
        let peer_id = self.peer_id.clone();
        drop(self);  // Just to be explicit
        peer_id
    }
}

impl<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId> fmt::Debug for
    CollectionReachEvent<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
where
    TPeerId: Eq + Hash + Clone + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("CollectionReachEvent")
            .field("peer_id", &self.peer_id)
            .field("reach_attempt_id", &self.reach_attempt_id())
            .finish()
    }
}

impl<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId> Drop for
    CollectionReachEvent<'a, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
{
    fn drop(&mut self) {
        let task = self.parent.inner.task(self.id)
            .expect("we create the CollectionReachEvent with a valid task id; the \
                     CollectionReachEvent mutably borrows the collection, therefore nothing \
                     can delete this task during the lifetime of the CollectionReachEvent; \
                     therefore the task is still valid when we delete it; QED");
        debug_assert!(if let TaskState::Pending = task.user_data() { true } else { false });
        task.close();
    }
}

/// Outcome of accepting a node.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CollectionNodeAccept<TUserData> {
    /// We replaced an existing node. Returns the user data that was assigned to this node.
    ReplacedExisting(TUserData),
    /// We didn't replace anything existing.
    NewEntry,
}

/// Identifier for a future that attempts to reach a node.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReachAttemptId(TaskId);

impl<TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
    CollectionStream<TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>
where
    TPeerId: Eq + Hash + Clone,
{
    /// Creates a new empty collection.
    #[inline]
    pub fn new() -> Self {
        CollectionStream {
            inner: HandledNodesTasks::new(),
            nodes: Default::default(),
        }
    }

    /// Adds to the collection a future that tries to reach a remote.
    ///
    /// This method spawns a task dedicated to resolving this future and processing the node's
    /// events.
    pub fn add_reach_attempt<TFut, TMuxer>(&mut self, future: TFut, handler: THandler)
        -> ReachAttemptId
    where
        TFut: Future<Item = (TPeerId, TMuxer), Error = TReachErr> + Send + 'static,
        THandler: IntoNodeHandler<TPeerId> + Send + 'static,
        THandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
        <THandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static,     // TODO: shouldn't be required?
        TReachErr: error::Error + Send + 'static,
        THandlerErr: error::Error + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,  // TODO: Send + Sync + 'static shouldn't be required
        TMuxer::OutboundSubstream: Send + 'static,  // TODO: shouldn't be required
        TPeerId: Send + 'static,
    {
        ReachAttemptId(self.inner.add_reach_attempt(future, TaskState::Pending, handler))
    }

    /// Interrupts a reach attempt.
    ///
    /// Returns `Ok` if something was interrupted, and `Err` if the ID is not or no longer valid.
    pub fn interrupt(&mut self, id: ReachAttemptId) -> Result<InterruptedReachAttempt<TInEvent, TPeerId, TUserData>, InterruptError> {
        match self.inner.task(id.0) {
            None => Err(InterruptError::ReachAttemptNotFound),
            Some(task) => {
                match task.user_data() {
                    TaskState::Connected(_, _) => return Err(InterruptError::AlreadyReached),
                    TaskState::Pending => (),
                };

                Ok(InterruptedReachAttempt {
                    inner: task.close(),
                })
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
    pub fn peer_mut(&mut self, id: &TPeerId) -> Option<PeerMut<'_, TInEvent, TUserData, TPeerId>> {
        let task = match self.nodes.get(id) {
            Some(&task) => task,
            None => return None,
        };

        match self.inner.task(task) {
            Some(inner) => Some(PeerMut {
                inner,
                nodes: &mut self.nodes,
            }),
            None => None,
        }
    }

    /// Returns true if we are connected to the given peer.
    ///
    /// This will return true only after a `NodeReached` event has been produced by `poll()`.
    #[inline]
    pub fn has_connection(&self, id: &TPeerId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Returns a list of all the active connections.
    ///
    /// Does not include reach attempts that haven't reached any target yet.
    #[inline]
    pub fn connections(&self) -> impl Iterator<Item = &TPeerId> {
        self.nodes.keys()
    }

    /// Provides an API similar to `Stream`, except that it cannot error.
    ///
    /// > **Note**: we use a regular `poll` method instead of implementing `Stream` in order to
    /// > remove the `Err` variant, but also because we want the `CollectionStream` to stay
    /// > borrowed if necessary.
    pub fn poll(&mut self) -> Async<CollectionEvent<'_, TInEvent, TOutEvent, THandler, TReachErr, THandlerErr, TUserData, TPeerId>> {
        let item = match self.inner.poll() {
            Async::Ready(item) => item,
            Async::NotReady => return Async::NotReady,
        };

        match item {
            HandledNodesEvent::TaskClosed { task, result, handler } => {
                let id = task.id();
                let user_data = task.into_user_data();

                match (user_data, result, handler) {
                    (TaskState::Pending, TaskClosedEvent::Reach(err), Some(handler)) => {
                        Async::Ready(CollectionEvent::ReachError {
                            id: ReachAttemptId(id),
                            error: err,
                            handler,
                        })
                    },
                    (TaskState::Pending, TaskClosedEvent::Node(_), _) => {
                        panic!("We switch the task state to Connected once we're connected, and \
                                a TaskClosedEvent::Node can only happen after we're \
                                connected; QED");
                    },
                    (TaskState::Pending, TaskClosedEvent::Reach(_), None) => {
                        // TODO: this could be improved in the API of HandledNodesTasks
                        panic!("The HandledNodesTasks is guaranteed to always return the handler \
                                when producing a TaskClosedEvent::Reach error");
                    },
                    (TaskState::Connected(peer_id, user_data), TaskClosedEvent::Node(err), _handler) => {
                        debug_assert!(_handler.is_none());
                        let _node_task_id = self.nodes.remove(&peer_id);
                        debug_assert_eq!(_node_task_id, Some(id));
                        Async::Ready(CollectionEvent::NodeClosed {
                            peer_id,
                            error: err,
                            user_data,
                        })
                    },
                    (TaskState::Connected(_, _), TaskClosedEvent::Reach(_), _) => {
                        panic!("A TaskClosedEvent::Reach can only happen before we are connected \
                                to a node; therefore the TaskState won't be Connected; QED");
                    },
                }
            },
            HandledNodesEvent::NodeReached { task, peer_id } => {
                let id = task.id();
                drop(task);
                Async::Ready(CollectionEvent::NodeReached(CollectionReachEvent {
                    parent: self,
                    id,
                    peer_id,
                }))
            },
            HandledNodesEvent::NodeEvent { task, event } => {
                let peer_id = match task.user_data() {
                    TaskState::Connected(peer_id, _) => peer_id.clone(),
                    _ => panic!("we can only receive NodeEvent events from a task after we \
                                 received a corresponding NodeReached event from that same task; \
                                 when we receive a NodeReached event, we ensure that the entry in \
                                 self.tasks is switched to the Connected state; QED"),
                };
                drop(task);
                Async::Ready(CollectionEvent::NodeEvent {
                    // TODO: normally we'd build a `PeerMut` manually here, but the borrow checker
                    //       doesn't like it
                    peer: self.peer_mut(&peer_id)
                        .expect("we can only receive NodeEvent events from a task after we \
                                 received a corresponding NodeReached event from that same task;\
                                 when that happens, peer_mut will always return Some; QED"),
                    event,
                })
            }
        }
    }
}

/// Reach attempt interrupt errors.
#[derive(Debug)]
pub enum InterruptError {
    /// An invalid reach attempt has been used to try to interrupt. The task
    /// entry is vacant; it needs to be added first via add_reach_attempt
    /// (with the TaskState set to Pending) before we try to connect.
    ReachAttemptNotFound,
    /// The task has already connected to the node; interrupting a reach attempt
    /// is thus redundant as it has already completed. Thus, the reach attempt
    /// that has tried to be used is no longer valid, since already reached.
    AlreadyReached,
}

impl fmt::Display for InterruptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            InterruptError::ReachAttemptNotFound =>
                write!(f, "The reach attempt could not be found."),
            InterruptError::AlreadyReached =>
                write!(f, "The reach attempt has already completed or reached the node."),
        }
    }
}

impl error::Error for InterruptError {}

/// Reach attempt after it has been interrupted.
pub struct InterruptedReachAttempt<TInEvent, TPeerId, TUserData> {
    inner: ClosedTask<TInEvent, TaskState<TPeerId, TUserData>>,
}

impl<TInEvent, TPeerId, TUserData> fmt::Debug for InterruptedReachAttempt<TInEvent, TPeerId, TUserData>
where
    TUserData: fmt::Debug,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_tuple("InterruptedReachAttempt")
            .field(&self.inner)
            .finish()
    }
}

/// Access to a peer in the collection.
pub struct PeerMut<'a, TInEvent, TUserData, TPeerId = PeerId> {
    inner: HandledNodesTask<'a, TInEvent, TaskState<TPeerId, TUserData>>,
    nodes: &'a mut FnvHashMap<TPeerId, TaskId>,
}

impl<'a, TInEvent, TUserData, TPeerId> PeerMut<'a, TInEvent, TUserData, TPeerId>
where
    TPeerId: Eq + Hash,
{
    /// Returns the identifier of the peer.
    pub fn id(&self) -> &TPeerId {
        match self.inner.user_data() {
            TaskState::Connected(peer_id, _) => peer_id,
            _ => panic!("A PeerMut is only ever constructed from a peer in the connected \
                         state; QED")
        }
    }

    /// Returns the user data that was stored in the collections when we accepted the connection.
    pub fn user_data(&self) -> &TUserData {
        match self.inner.user_data() {
            TaskState::Connected(_, user_data) => user_data,
            _ => panic!("A PeerMut is only ever constructed from a peer in the connected \
                         state; QED")
        }
    }

    /// Returns the user data that was stored in the collections when we accepted the connection.
    pub fn user_data_mut(&mut self) -> &mut TUserData {
        match self.inner.user_data_mut() {
            TaskState::Connected(_, user_data) => user_data,
            _ => panic!("A PeerMut is only ever constructed from a peer in the connected \
                         state; QED")
        }
    }

    /// Sends an event to the given node.
    #[inline]
    pub fn send_event(&mut self, event: TInEvent) {
        self.inner.send_event(event)
    }

    /// Closes the connections to this node. Returns the user data.
    ///
    /// No further event will be generated for this node.
    pub fn close(self) -> TUserData {
        let task_id = self.inner.id();
        if let TaskState::Connected(peer_id, user_data) = self.inner.close().into_user_data() {
            let old_task_id = self.nodes.remove(&peer_id);
            debug_assert_eq!(old_task_id, Some(task_id));
            user_data
        } else {
            panic!("a PeerMut can only be created if an entry is present in nodes; an entry in \
                    nodes always matched a Connected entry in the tasks; QED");
        }
    }

    /// Gives ownership of a closed reach attempt. As soon as the connection to the peer (`self`)
    /// has some acknowledgment from the remote that its connection is alive, it will close the
    /// connection inside `id`.
    ///
    /// The reach attempt will only be effectively cancelled once the peer (the object you're
    /// manipulating) has received some network activity. However no event will be ever be
    /// generated from this reach attempt, and this takes effect immediately.
    pub fn take_over(&mut self, id: InterruptedReachAttempt<TInEvent, TPeerId, TUserData>) {
        let _state = self.inner.take_over(id.inner);
        debug_assert!(if let TaskState::Pending = _state { true } else { false });
    }
}
