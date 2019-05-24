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
        handled_node::{HandledNode, HandledNodeError, NodeHandler},
        node::{Close, Substream}
    }
};
use fnv::FnvHashMap;
use futures::{prelude::*, stream, sync::mpsc};
use smallvec::SmallVec;
use std::{
    collections::hash_map::{Entry, OccupiedEntry},
    error,
    fmt,
    mem
};
use tokio_executor::Executor;

mod tests;

// Implementor notes
// =================
//
// This collection of nodes spawns a task for each individual node to process. This means that
// events happen on the background at the same time as the `HandledNodesTasks` is being polled.
//
// In order to make the API non-racy and avoid issues, we completely separate the state in the
// `HandledNodesTasks` from the states that the task nodes can access. They are only allowed to
// exchange messages. The state in the `HandledNodesTasks` is therefore delayed compared to the
// tasks, and is updated only when `poll()` is called.
//
// The only thing that we must be careful about is substreams, as they are "detached" from the
// state of the `HandledNodesTasks` and allowed to process in parallel. This is why there is no
// "substream closed" event being reported, as it could potentially create confusions and race
// conditions in the user's code. See similar comments in the documentation of `NodeStream`.

/// Implementation of `Stream` that handles a collection of nodes.
pub struct HandledNodesTasks<TInEvent, TOutEvent, TIntoHandler, TReachErr, THandlerErr, TUserData, TConnInfo = PeerId> {
    /// A map between active tasks to an unbounded sender, used to control the task. Closing the sender interrupts
    /// the task. It is possible that we receive messages from tasks that used to be in this list
    /// but no longer are, in which case we should ignore them.
    tasks: FnvHashMap<TaskId, (mpsc::UnboundedSender<ExtToInMessage<TInEvent>>, TUserData)>,

    /// Identifier for the next task to spawn.
    next_task_id: TaskId,

    /// List of node tasks to spawn.
    // TODO: stronger typing?
    to_spawn: SmallVec<[Box<dyn Future<Item = (), Error = ()> + Send>; 8]>,
    /// If no tokio executor is available, we move tasks to this list, and futures are polled on
    /// the current thread instead.
    local_spawns: Vec<Box<dyn Future<Item = (), Error = ()> + Send>>,

    /// Sender to emit events to the outside. Meant to be cloned and sent to tasks.
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TOutEvent, TIntoHandler, TReachErr, THandlerErr, TConnInfo>, TaskId)>,
    /// Receiver side for the events.
    events_rx: mpsc::UnboundedReceiver<(InToExtMessage<TOutEvent, TIntoHandler, TReachErr, THandlerErr, TConnInfo>, TaskId)>,
}

impl<TInEvent, TOutEvent, TIntoHandler, TReachErr, THandlerErr, TUserData, TConnInfo> fmt::Debug for
    HandledNodesTasks<TInEvent, TOutEvent, TIntoHandler, TReachErr, THandlerErr, TUserData, TConnInfo>
where
    TUserData: fmt::Debug
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_map()
            .entries(self.tasks.iter().map(|(id, (_, ud))| (id, ud)))
            .finish()
    }
}

/// Error that can happen in a task.
#[derive(Debug)]
pub enum TaskClosedEvent<TReachErr, THandlerErr> {
    /// An error happend while we were trying to reach the node.
    Reach(TReachErr),
    /// An error happened after the node has been reached.
    Node(HandledNodeError<THandlerErr>),
}

impl<TReachErr, THandlerErr> fmt::Display for TaskClosedEvent<TReachErr, THandlerErr>
where
    TReachErr: fmt::Display,
    THandlerErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskClosedEvent::Reach(err) => write!(f, "{}", err),
            TaskClosedEvent::Node(err) => write!(f, "{}", err),
        }
    }
}

impl<TReachErr, THandlerErr> error::Error for TaskClosedEvent<TReachErr, THandlerErr>
where
    TReachErr: error::Error + 'static,
    THandlerErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            TaskClosedEvent::Reach(err) => Some(err),
            TaskClosedEvent::Node(err) => Some(err),
        }
    }
}

/// Prototype for a `NodeHandler`.
pub trait IntoNodeHandler<TConnInfo = PeerId> {
    /// The node handler.
    type Handler: NodeHandler;

    /// Builds the node handler.
    ///
    /// The `TConnInfo` is the information about the connection that the handler is going to handle.
    /// This is generated by the `Transport` and typically implements the `ConnectionInfo` trait.
    fn into_handler(self, remote_conn_info: &TConnInfo) -> Self::Handler;
}

impl<T, TConnInfo> IntoNodeHandler<TConnInfo> for T
where T: NodeHandler
{
    type Handler = Self;

    #[inline]
    fn into_handler(self, _: &TConnInfo) -> Self {
        self
    }
}

/// Event that can happen on the `HandledNodesTasks`.
#[derive(Debug)]
pub enum HandledNodesEvent<'a, TInEvent, TOutEvent, TIntoHandler, TReachErr, THandlerErr, TUserData, TConnInfo = PeerId> {
    /// A task has been closed.
    ///
    /// This happens once the node handler closes or an error happens.
    // TODO: send back undelivered events?
    TaskClosed {
        /// The task that has been closed.
        task: ClosedTask<TInEvent, TUserData>,
        /// What happened.
        result: TaskClosedEvent<TReachErr, THandlerErr>,
        /// If the task closed before reaching the node, this contains the handler that was passed
        /// to `add_reach_attempt`.
        handler: Option<TIntoHandler>,
    },

    /// A task has successfully connected to a node.
    NodeReached {
        /// The task that succeeded.
        task: Task<'a, TInEvent, TUserData>,
        /// Identifier of the node.
        conn_info: TConnInfo,
    },

    /// A task has produced an event.
    NodeEvent {
        /// The task that produced the event.
        task: Task<'a, TInEvent, TUserData>,
        /// The produced event.
        event: TOutEvent,
    },
}

/// Identifier for a future that attempts to reach a node.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(usize);

impl<TInEvent, TOutEvent, TIntoHandler, TReachErr, THandlerErr, TUserData, TConnInfo>
    HandledNodesTasks<TInEvent, TOutEvent, TIntoHandler, TReachErr, THandlerErr, TUserData, TConnInfo>
{
    /// Creates a new empty collection.
    #[inline]
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::unbounded();

        HandledNodesTasks {
            tasks: Default::default(),
            next_task_id: TaskId(0),
            to_spawn: SmallVec::new(),
            local_spawns: Vec::new(),
            events_tx,
            events_rx,
        }
    }

    /// Adds to the collection a future that tries to reach a node.
    ///
    /// This method spawns a task dedicated to resolving this future and processing the node's
    /// events.
    pub fn add_reach_attempt<TFut, TMuxer>(&mut self, future: TFut, user_data: TUserData, handler: TIntoHandler) -> TaskId
    where
        TFut: Future<Item = (TConnInfo, TMuxer), Error = TReachErr> + Send + 'static,
        TIntoHandler: IntoNodeHandler<TConnInfo> + Send + 'static,
        TIntoHandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent, Error = THandlerErr> + Send + 'static,
        TReachErr: error::Error + Send + 'static,
        THandlerErr: error::Error + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        <TIntoHandler::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static,     // TODO: shouldn't be required?
        TMuxer: StreamMuxer + Send + Sync + 'static,  // TODO: Send + Sync + 'static shouldn't be required
        TMuxer::OutboundSubstream: Send + 'static,  // TODO: shouldn't be required
        TConnInfo: Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::unbounded();
        self.tasks.insert(task_id, (tx, user_data));

        let task = Box::new(NodeTask {
            taken_over: SmallVec::new(),
            inner: NodeTaskInner::Future {
                future,
                handler,
                events_buffer: Vec::new(),
            },
            events_tx: self.events_tx.clone(),
            in_events_rx: rx.fuse(),
            id: task_id,
        });

        self.to_spawn.push(task);
        task_id
    }

    /// Sends an event to all the tasks, including the pending ones.
    pub fn broadcast_event(&mut self, event: &TInEvent)
    where TInEvent: Clone,
    {
        for (sender, _) in self.tasks.values() {
            // Note: it is possible that sending an event fails if the background task has already
            // finished, but the local state hasn't reflected that yet because it hasn't been
            // polled. This is not an error situation.
            let _ = sender.unbounded_send(ExtToInMessage::HandlerEvent(event.clone()));
        }
    }

    /// Grants access to an object that allows controlling a task of the collection.
    ///
    /// Returns `None` if the task id is invalid.
    #[inline]
    pub fn task(&mut self, id: TaskId) -> Option<Task<'_, TInEvent, TUserData>> {
        match self.tasks.entry(id) {
            Entry::Occupied(inner) => Some(Task { inner }),
            Entry::Vacant(_) => None,
        }
    }

    /// Returns a list of all the active tasks.
    #[inline]
    pub fn tasks<'a>(&'a self) -> impl Iterator<Item = TaskId> + 'a {
        self.tasks.keys().cloned()
    }

    /// Provides an API similar to `Stream`, except that it cannot produce an error.
    pub fn poll(&mut self) -> Async<HandledNodesEvent<TInEvent, TOutEvent, TIntoHandler, TReachErr, THandlerErr, TUserData, TConnInfo>> {
        let (message, task_id) = match self.poll_inner() {
            Async::Ready(r) => r,
            Async::NotReady => return Async::NotReady,
        };

        Async::Ready(match message {
            InToExtMessage::NodeEvent(event) => {
                HandledNodesEvent::NodeEvent {
                    task: match self.tasks.entry(task_id) {
                        Entry::Occupied(inner) => Task { inner },
                        Entry::Vacant(_) => panic!("poll_inner only returns valid TaskIds; QED")
                    },
                    event
                }
            },
            InToExtMessage::NodeReached(conn_info) => {
                HandledNodesEvent::NodeReached {
                    task: match self.tasks.entry(task_id) {
                        Entry::Occupied(inner) => Task { inner },
                        Entry::Vacant(_) => panic!("poll_inner only returns valid TaskIds; QED")
                    },
                    conn_info
                }
            },
            InToExtMessage::TaskClosed(result, handler) => {
                let (channel, user_data) = self.tasks.remove(&task_id)
                    .expect("poll_inner only returns valid TaskIds; QED");
                HandledNodesEvent::TaskClosed {
                    task: ClosedTask {
                        id: task_id,
                        channel,
                        user_data,
                    },
                    result,
                    handler,
                }
            },
        })
    }

    /// Since non-lexical lifetimes still don't work very well in Rust at the moment, we have to
    /// split `poll()` in two. This method returns an `InToExtMessage` that is guaranteed to come
    /// from an alive task.
    // TODO: look into merging with `poll()`
    fn poll_inner(&mut self) -> Async<(InToExtMessage<TOutEvent, TIntoHandler, TReachErr, THandlerErr, TConnInfo>, TaskId)> {
        for to_spawn in self.to_spawn.drain() {
            // We try to use the default executor, but fall back to polling the task manually if
            // no executor is available. This makes it possible to use the core in environments
            // outside of tokio.
            let mut executor = tokio_executor::DefaultExecutor::current();
            if executor.status().is_ok() {
                executor.spawn(to_spawn).expect("failed to create a node task");
            } else {
                self.local_spawns.push(to_spawn);
            }
        }

        for n in (0..self.local_spawns.len()).rev() {
            let mut task = self.local_spawns.swap_remove(n);
            match task.poll() {
                Ok(Async::Ready(())) => {},
                Ok(Async::NotReady) => self.local_spawns.push(task),
                // It would normally be desirable to either report or log when a background task
                // errors. However the default tokio executor doesn't do anything in case of error,
                // and therefore we mimic this behaviour by also not doing anything.
                Err(()) => {}
            }
        }

        loop {
            match self.events_rx.poll() {
                Ok(Async::Ready(Some((message, task_id)))) => {
                    // If the task id is no longer in `self.tasks`, that means that the user called
                    // `close()` on this task earlier. Therefore no new event should be generated
                    // for this task.
                    if self.tasks.contains_key(&task_id) {
                        break Async::Ready((message, task_id));
                    }
                }
                Ok(Async::NotReady) => {
                    break Async::NotReady;
                }
                Ok(Async::Ready(None)) => {
                    unreachable!("The sender is in self as well, therefore the receiver never \
                                  closes.")
                },
                Err(()) => unreachable!("An unbounded receiver never errors"),
            }
        }
    }
}

/// Access to a task in the collection.
pub struct Task<'a, TInEvent, TUserData> {
    inner: OccupiedEntry<'a, TaskId, (mpsc::UnboundedSender<ExtToInMessage<TInEvent>>, TUserData)>,
}

impl<'a, TInEvent, TUserData> Task<'a, TInEvent, TUserData> {
    /// Sends an event to the given node.
    // TODO: report back on delivery
    #[inline]
    pub fn send_event(&mut self, event: TInEvent) {
        // It is possible that the sender is closed if the background task has already finished
        // but the local state hasn't been updated yet because we haven't been polled in the
        // meanwhile.
        let _ = self.inner.get_mut().0.unbounded_send(ExtToInMessage::HandlerEvent(event));
    }

    /// Returns the user data associated with the task.
    pub fn user_data(&self) -> &TUserData {
        &self.inner.get().1
    }

    /// Returns the user data associated with the task.
    pub fn user_data_mut(&mut self) -> &mut TUserData {
        &mut self.inner.get_mut().1
    }

    /// Returns the task id.
    #[inline]
    pub fn id(&self) -> TaskId {
        *self.inner.key()
    }

    /// Closes the task. Returns the user data.
    ///
    /// No further event will be generated for this task, but the connection inside the task will
    /// continue to run until the `ClosedTask` is destroyed.
    pub fn close(self) -> ClosedTask<TInEvent, TUserData> {
        let id = *self.inner.key();
        let (channel, user_data) = self.inner.remove();
        ClosedTask { id, channel, user_data }
    }

    /// Gives ownership of a closed task. As soon as our task (`self`) has some acknowledgment from
    /// the remote that its connection is alive, it will close the connection with `other`.
    pub fn take_over(&mut self, other: ClosedTask<TInEvent, TUserData>) -> TUserData {
        // It is possible that the sender is closed if the background task has already finished
        // but the local state hasn't been updated yet because we haven't been polled in the
        // meanwhile.
        let _ = self.inner.get_mut().0.unbounded_send(ExtToInMessage::TakeOver(other.channel));
        other.user_data
    }
}

impl<'a, TInEvent, TUserData> fmt::Debug for Task<'a, TInEvent, TUserData>
where
    TUserData: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_tuple("Task")
            .field(&self.id())
            .field(self.user_data())
            .finish()
    }
}

/// Task after it has been closed. The connection to the remote is potentially still going on, but
/// no new event for this task will be received.
pub struct ClosedTask<TInEvent, TUserData> {
    /// Identifier of the task that closed. No longer corresponds to anything, but can be reported
    /// to the user.
    id: TaskId,
    /// The channel to the task. The task will continue to work for as long as this channel is
    /// alive, but events produced by it are ignored.
    channel: mpsc::UnboundedSender<ExtToInMessage<TInEvent>>,
    /// The data provided by the user.
    user_data: TUserData,
}

impl<TInEvent, TUserData> ClosedTask<TInEvent, TUserData> {
    /// Returns the task id. Note that this task is no longer part of the collection, and therefore
    /// calling `task()` with this ID will fail.
    #[inline]
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the user data associated with the task.
    pub fn user_data(&self) -> &TUserData {
        &self.user_data
    }

    /// Returns the user data associated with the task.
    pub fn user_data_mut(&mut self) -> &mut TUserData {
        &mut self.user_data
    }

    /// Finish destroying the task and yield the user data. This closes the connection to the
    /// remote.
    pub fn into_user_data(self) -> TUserData {
        self.user_data
    }
}

impl<TInEvent, TUserData> fmt::Debug for ClosedTask<TInEvent, TUserData>
where
    TUserData: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_tuple("ClosedTask")
            .field(&self.id)
            .field(&self.user_data)
            .finish()
    }
}

/// Message to transmit from the public API to a task.
#[derive(Debug)]
enum ExtToInMessage<TInEvent> {
    /// An event to transmit to the node handler.
    HandlerEvent(TInEvent),
    /// When received, stores the parameter inside the task and keeps it alive until we have an
    /// acknowledgment that the remote has accepted our handshake.
    TakeOver(mpsc::UnboundedSender<ExtToInMessage<TInEvent>>),
}

/// Message to transmit from a task to the public API.
#[derive(Debug)]
enum InToExtMessage<TOutEvent, TIntoHandler, TReachErr, THandlerErr, TConnInfo> {
    /// A connection to a node has succeeded.
    NodeReached(TConnInfo),
    /// The task closed.
    TaskClosed(TaskClosedEvent<TReachErr, THandlerErr>, Option<TIntoHandler>),
    /// An event from the node.
    NodeEvent(TOutEvent),
}

/// Implementation of `Future` that handles a single node, and all the communications between
/// the various components of the `HandledNodesTasks`.
struct NodeTask<TFut, TMuxer, TIntoHandler, TInEvent, TOutEvent, TReachErr, TConnInfo>
where
    TMuxer: StreamMuxer,
    TIntoHandler: IntoNodeHandler<TConnInfo>,
    TIntoHandler::Handler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Sender to transmit events to the outside.
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TOutEvent, TIntoHandler, TReachErr, <TIntoHandler::Handler as NodeHandler>::Error, TConnInfo>, TaskId)>,
    /// Receiving end for events sent from the main `HandledNodesTasks`.
    in_events_rx: stream::Fuse<mpsc::UnboundedReceiver<ExtToInMessage<TInEvent>>>,
    /// Inner state of the `NodeTask`.
    inner: NodeTaskInner<TFut, TMuxer, TIntoHandler, TInEvent, TConnInfo>,
    /// Identifier of the attempt.
    id: TaskId,
    /// Channels to keep alive for as long as we don't have an acknowledgment from the remote.
    taken_over: SmallVec<[mpsc::UnboundedSender<ExtToInMessage<TInEvent>>; 1]>,
}

enum NodeTaskInner<TFut, TMuxer, TIntoHandler, TInEvent, TConnInfo>
where
    TMuxer: StreamMuxer,
    TIntoHandler: IntoNodeHandler<TConnInfo>,
    TIntoHandler::Handler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Future to resolve to connect to the node.
    Future {
        /// The future that will attempt to reach the node.
        future: TFut,
        /// The handler that will be used to build the `HandledNode`.
        handler: TIntoHandler,
        /// While we are dialing the future, we need to buffer the events received on
        /// `in_events_rx` so that they get delivered once dialing succeeds. We can't simply leave
        /// events in `in_events_rx` because we have to detect if it gets closed.
        events_buffer: Vec<TInEvent>,
    },

    /// Fully functional node.
    Node(HandledNode<TMuxer, TIntoHandler::Handler>),

    /// Node closing.
    Closing(Close<TMuxer>),

    /// A panic happened while polling.
    Poisoned,
}

impl<TFut, TMuxer, TIntoHandler, TInEvent, TOutEvent, TReachErr, TConnInfo> Future for
    NodeTask<TFut, TMuxer, TIntoHandler, TInEvent, TOutEvent, TReachErr, TConnInfo>
where
    TMuxer: StreamMuxer,
    TFut: Future<Item = (TConnInfo, TMuxer), Error = TReachErr>,
    TIntoHandler: IntoNodeHandler<TConnInfo>,
    TIntoHandler::Handler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent>,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        'outer_loop: loop {
            match mem::replace(&mut self.inner, NodeTaskInner::Poisoned) {
                // First possibility: we are still trying to reach a node.
                NodeTaskInner::Future { mut future, handler, mut events_buffer } => {
                    // If self.in_events_rx is closed, we stop the task.
                    loop {
                        match self.in_events_rx.poll() {
                            Ok(Async::Ready(None)) => return Ok(Async::Ready(())),
                            Ok(Async::Ready(Some(ExtToInMessage::HandlerEvent(event)))) => {
                                events_buffer.push(event)
                            },
                            Ok(Async::Ready(Some(ExtToInMessage::TakeOver(take_over)))) => {
                                self.taken_over.push(take_over);
                            },
                            Ok(Async::NotReady) => break,
                            Err(_) => unreachable!("An UnboundedReceiver never errors"),
                        }
                    }
                    // Check whether dialing succeeded.
                    match future.poll() {
                        Ok(Async::Ready((conn_info, muxer))) => {
                            let mut node = HandledNode::new(muxer, handler.into_handler(&conn_info));
                            let event = InToExtMessage::NodeReached(conn_info);
                            for event in events_buffer {
                                node.inject_event(event);
                            }
                            let _ = self.events_tx.unbounded_send((event, self.id));
                            self.inner = NodeTaskInner::Node(node);
                        }
                        Ok(Async::NotReady) => {
                            self.inner = NodeTaskInner::Future { future, handler, events_buffer };
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            // End the task
                            let event = InToExtMessage::TaskClosed(TaskClosedEvent::Reach(err), Some(handler));
                            let _ = self.events_tx.unbounded_send((event, self.id));
                            return Ok(Async::Ready(()));
                        }
                    }
                },

                // Second possibility: we have a node.
                NodeTaskInner::Node(mut node) => {
                    // Start by handling commands received from the outside of the task.
                    if !self.in_events_rx.is_done() {
                        loop {
                            match self.in_events_rx.poll() {
                                Ok(Async::NotReady) => break,
                                Ok(Async::Ready(Some(ExtToInMessage::HandlerEvent(event)))) => {
                                    node.inject_event(event)
                                },
                                Ok(Async::Ready(Some(ExtToInMessage::TakeOver(take_over)))) => {
                                    self.taken_over.push(take_over);
                                },
                                Ok(Async::Ready(None)) => {
                                    // Node closed by the external API; start closing.
                                    self.inner = NodeTaskInner::Closing(node.close());
                                    continue 'outer_loop;
                                }
                                Err(()) => unreachable!("An unbounded receiver never errors"),
                            }
                        }
                    }

                    // Process the node.
                    loop {
                        if !self.taken_over.is_empty() && node.is_remote_acknowledged() {
                            self.taken_over.clear();
                        }

                        match node.poll() {
                            Ok(Async::NotReady) => {
                                self.inner = NodeTaskInner::Node(node);
                                return Ok(Async::NotReady);
                            },
                            Ok(Async::Ready(event)) => {
                                let event = InToExtMessage::NodeEvent(event);
                                let _ = self.events_tx.unbounded_send((event, self.id));
                            }
                            Err(err) => {
                                let event = InToExtMessage::TaskClosed(TaskClosedEvent::Node(err), None);
                                let _ = self.events_tx.unbounded_send((event, self.id));
                                return Ok(Async::Ready(())); // End the task.
                            }
                        }
                    }
                },

                NodeTaskInner::Closing(mut closing) => {
                    match closing.poll() {
                        Ok(Async::Ready(())) | Err(_) => {
                            return Ok(Async::Ready(())); // End the task.
                        },
                        Ok(Async::NotReady) => {
                            self.inner = NodeTaskInner::Closing(closing);
                            return Ok(Async::NotReady);
                        }
                    }
                },

                // This happens if a previous poll has ended unexpectedly. The API of futures
                // guarantees that we shouldn't be polled again.
                NodeTaskInner::Poisoned => panic!("the node task panicked or errored earlier")
            }
        }
    }
}
