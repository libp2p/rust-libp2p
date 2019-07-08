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
        handled_node::{HandledNode, IntoNodeHandler, NodeHandler},
        node::Substream
    }
};
use fnv::FnvHashMap;
use futures::{prelude::*, future::Executor, sync::mpsc};
use smallvec::SmallVec;
use std::{collections::hash_map::{Entry, OccupiedEntry}, error, fmt};
use super::{TaskId, task::{Task, FromTaskMessage, ToTaskMessage}, Error};

// Implementor notes
// =================
//
// This collection of nodes spawns a `Task` for each individual node to process.
// This means that events happen asynchronously at the same time as the
// `Manager` is being polled.
//
// In order to make the API non-racy and avoid issues, we completely separate
// the state in the `Manager` from the states that the `Task` can access.
// They are only allowed to exchange messages. The state in the `Manager` is
// therefore delayed compared to the tasks, and is updated only when `poll()`
// is called.
//
// The only thing that we must be careful about is substreams, as they are
// "detached" from the state of the `Manager` and allowed to process
// concurrently. This is why there is no "substream closed" event being
// reported, as it could potentially create confusions and race conditions in
// the user's code. See similar comments in the documentation of `NodeStream`.
//

/// Implementation of [`Stream`] that handles a collection of nodes.
pub struct Manager<I, O, H, E, HE, T, C = PeerId> {
    /// Collection of managed tasks.
    ///
    /// Closing the sender interrupts the task. It is possible that we receive
    /// messages from tasks that used to be in this collection but no longer
    /// are, in which case we should ignore them.
    tasks: FnvHashMap<TaskId, TaskInfo<I, T>>,

    /// Identifier for the next task to spawn.
    next_task_id: TaskId,

    /// List of node tasks to spawn.
    to_spawn: SmallVec<[Box<dyn Future<Item = (), Error = ()> + Send>; 8]>,

    /// If no tokio executor is available, we move tasks to this list, and futures are polled on
    /// the current thread instead.
    local_spawns: Vec<Box<dyn Future<Item = (), Error = ()> + Send>>,

    /// Sender to emit events to the outside. Meant to be cloned and sent to tasks.
    events_tx: mpsc::Sender<(FromTaskMessage<O, H, E, HE, C>, TaskId)>,

    /// Receiver side for the events.
    events_rx: mpsc::Receiver<(FromTaskMessage<O, H, E, HE, C>, TaskId)>
}

impl<I, O, H, E, HE, T, C> fmt::Debug for Manager<I, O, H, E, HE, T, C>
where
    T: fmt::Debug
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_map()
            .entries(self.tasks.iter().map(|(id, task)| (id, &task.user_data)))
            .finish()
    }
}

/// Information about a running task.
///
/// Contains the sender to deliver event messages to the task,
/// the associated user data and a pending message if any,
/// meant to be delivered to the task via the sender.
struct TaskInfo<I, T> {
    /// channel endpoint to send messages to the task
    sender: mpsc::Sender<ToTaskMessage<I>>,
    /// task associated data
    user_data: T,
    /// any pending event to deliver to the task
    pending: Option<AsyncSink<ToTaskMessage<I>>>
}

/// Event produced by the [`Manager`].
#[derive(Debug)]
pub enum Event<'a, I, O, H, E, HE, T, C = PeerId> {
    /// A task has been closed.
    ///
    /// This happens once the node handler closes or an error happens.
    TaskClosed {
        /// The task that has been closed.
        task: ClosedTask<I, T>,
        /// What happened.
        result: Error<E, HE>,
        /// If the task closed before reaching the node, this contains
        /// the handler that was passed to `add_reach_attempt`.
        handler: Option<H>
    },

    /// A task has successfully connected to a node.
    NodeReached {
        /// The task that succeeded.
        task: TaskEntry<'a, I, T>,
        /// Identifier of the node.
        conn_info: C
    },

    /// A task has produced an event.
    NodeEvent {
        /// The task that produced the event.
        task: TaskEntry<'a, I, T>,
        /// The produced event.
        event: O
    }
}

impl<I, O, H, E, HE, T, C> Manager<I, O, H, E, HE, T, C> {
    /// Creates a new task manager.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1);
        Self {
            tasks: FnvHashMap::default(),
            next_task_id: TaskId(0),
            to_spawn: SmallVec::new(),
            local_spawns: Vec::new(),
            events_tx: tx,
            events_rx: rx
        }
    }

    /// Adds to the manager a future that tries to reach a node.
    ///
    /// This method spawns a task dedicated to resolving this future and
    /// processing the node's events.
    pub fn add_reach_attempt<F, M>(&mut self, future: F, user_data: T, handler: H) -> TaskId
    where
        F: Future<Item = (C, M), Error = E> + Send + 'static,
        H: IntoNodeHandler<C> + Send + 'static,
        H::Handler: NodeHandler<Substream = Substream<M>, InEvent = I, OutEvent = O, Error = HE> + Send + 'static,
        E: error::Error + Send + 'static,
        HE: error::Error + Send + 'static,
        I: Send + 'static,
        O: Send + 'static,
        <H::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static,
        M: StreamMuxer + Send + Sync + 'static,
        M::OutboundSubstream: Send + 'static,
        C: Send + 'static
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::channel(4);
        self.tasks.insert(task_id, TaskInfo { sender: tx, user_data, pending: None });

        let task = Box::new(Task::new(task_id, self.events_tx.clone(), rx, future, handler));
        self.to_spawn.push(task);
        task_id
    }

    /// Adds an existing connection to a node to the collection.
    ///
    /// This method spawns a task dedicated to processing the node's events.
    ///
    /// No `NodeReached` event will be emitted for this task, since the node has already been
    /// reached.
    pub fn add_connection<M, Handler>(&mut self, user_data: T, muxer: M, handler: Handler) -> TaskId
    where
        H: IntoNodeHandler<C, Handler = Handler> + Send + 'static,
        Handler: NodeHandler<Substream = Substream<M>, InEvent = I, OutEvent = O, Error = HE> + Send + 'static,
        E: error::Error + Send + 'static,
        HE: error::Error + Send + 'static,
        I: Send + 'static,
        O: Send + 'static,
        <H::Handler as NodeHandler>::OutboundOpenInfo: Send + 'static,
        M: StreamMuxer + Send + Sync + 'static,
        M::OutboundSubstream: Send + 'static,
        C: Send + 'static
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::channel(4);
        self.tasks.insert(task_id, TaskInfo { sender: tx, user_data, pending: None });

        let task: Task<futures::future::Empty<_, _>, _, _, _, _, _, _> =
            Task::node(task_id, self.events_tx.clone(), rx, HandledNode::new(muxer, handler));

        self.to_spawn.push(Box::new(task));
        task_id
    }

    /// Start sending an event to all the tasks, including the pending ones.
    ///
    /// After starting a broadcast make sure to finish it with `complete_broadcast`,
    /// otherwise starting another broadcast or sending an event directly to a
    /// task would overwrite the pending broadcast.
    #[must_use]
    pub fn start_broadcast(&mut self, event: &I) -> AsyncSink<()>
    where
        I: Clone
    {
        if self.complete_broadcast().is_not_ready() {
            return AsyncSink::NotReady(())
        }

        for task in self.tasks.values_mut() {
            let msg = ToTaskMessage::HandlerEvent(event.clone());
            task.pending = Some(AsyncSink::NotReady(msg))
        }

        AsyncSink::Ready
    }

    /// Complete a started broadcast.
    #[must_use]
    pub fn complete_broadcast(&mut self) -> Async<()> {
        let mut ready = true;

        for task in self.tasks.values_mut() {
            match task.pending.take() {
                Some(AsyncSink::NotReady(msg)) =>
                    match task.sender.start_send(msg) {
                        Ok(AsyncSink::NotReady(msg)) => {
                            task.pending = Some(AsyncSink::NotReady(msg));
                            ready = false
                        }
                        Ok(AsyncSink::Ready) =>
                            if let Ok(Async::NotReady) = task.sender.poll_complete() {
                                task.pending = Some(AsyncSink::Ready);
                                ready = false
                            }
                        Err(_) => {}
                    }
                Some(AsyncSink::Ready) =>
                    if let Ok(Async::NotReady) = task.sender.poll_complete() {
                        task.pending = Some(AsyncSink::Ready);
                        ready = false
                    }
                None => {}
            }
        }

        if ready {
            Async::Ready(())
        } else {
            Async::NotReady
        }
    }

    /// Grants access to an object that allows controlling a task of the collection.
    ///
    /// Returns `None` if the task id is invalid.
    pub fn task(&mut self, id: TaskId) -> Option<TaskEntry<'_, I, T>> {
        match self.tasks.entry(id) {
            Entry::Occupied(inner) => Some(TaskEntry { inner }),
            Entry::Vacant(_) => None,
        }
    }

    /// Returns a list of all the active tasks.
    pub fn tasks<'a>(&'a self) -> impl Iterator<Item = TaskId> + 'a {
        self.tasks.keys().cloned()
    }

    /// Provides an API similar to `Stream`, except that it cannot produce an error.
    pub fn poll(&mut self) -> Async<Event<I, O, H, E, HE, T, C>> {
        for to_spawn in self.to_spawn.drain() {
            // We try to use the default executor, but fall back to polling the task manually if
            // no executor is available. This makes it possible to use the core in environments
            // outside of tokio.
            let executor = tokio_executor::DefaultExecutor::current();
            if let Err(err) = executor.execute(to_spawn) {
                self.local_spawns.push(err.into_future())
            }
        }

        for n in (0 .. self.local_spawns.len()).rev() {
            let mut task = self.local_spawns.swap_remove(n);
            match task.poll() {
                Ok(Async::Ready(())) => {}
                Ok(Async::NotReady) => self.local_spawns.push(task),
                // It would normally be desirable to either report or log when a background task
                // errors. However the default tokio executor doesn't do anything in case of error,
                // and therefore we mimic this behaviour by also not doing anything.
                Err(()) => {}
            }
        }

        let (message, task_id) = loop {
            match self.events_rx.poll() {
                Ok(Async::Ready(Some((message, task_id)))) => {
                    // If the task id is no longer in `self.tasks`, that means that the user called
                    // `close()` on this task earlier. Therefore no new event should be generated
                    // for this task.
                    if self.tasks.contains_key(&task_id) {
                        break (message, task_id)
                    }
                }
                Ok(Async::NotReady) => return Async::NotReady,
                Ok(Async::Ready(None)) => unreachable!("sender and receiver have same lifetime"),
                Err(()) => unreachable!("An `mpsc::Receiver` does not error.")
            }
        };

        Async::Ready(match message {
            FromTaskMessage::NodeEvent(event) =>
                Event::NodeEvent {
                    task: match self.tasks.entry(task_id) {
                        Entry::Occupied(inner) => TaskEntry { inner },
                        Entry::Vacant(_) => panic!("poll_inner only returns valid TaskIds; QED")
                    },
                    event
                },
            FromTaskMessage::NodeReached(conn_info) =>
                Event::NodeReached {
                    task: match self.tasks.entry(task_id) {
                        Entry::Occupied(inner) => TaskEntry { inner },
                        Entry::Vacant(_) => panic!("poll_inner only returns valid TaskIds; QED")
                    },
                    conn_info
                },
            FromTaskMessage::TaskClosed(result, handler) => {
                let entry = self.tasks.remove(&task_id)
                    .expect("poll_inner only returns valid TaskIds; QED");
                Event::TaskClosed {
                    task: ClosedTask::new(task_id, entry.sender, entry.user_data),
                    result,
                    handler
                }
            }
        })
    }
}

/// Access to a task in the collection.
pub struct TaskEntry<'a, E, T> {
    inner: OccupiedEntry<'a, TaskId, TaskInfo<E, T>>
}

impl<'a, E, T> TaskEntry<'a, E, T> {
    /// Begin sending an event to the given node.
    ///
    /// Make sure to finish the send operation with `complete_send_event`.
    pub fn start_send_event(&mut self, event: E) -> StartSend<E, ()> {
        let msg = ToTaskMessage::HandlerEvent(event);
        if let AsyncSink::NotReady(msg) = self.start_send_event_msg(msg)? {
            if let ToTaskMessage::HandlerEvent(event) = msg {
                return Ok(AsyncSink::NotReady(event))
            } else {
                unreachable!("we tried to send an handler event, so we get one back if not ready")
            }
        }
        Ok(AsyncSink::Ready)
    }

    /// Finish a send operation started with `start_send_event`.
    pub fn complete_send_event(&mut self) -> Poll<(), ()> {
        self.complete_send_event_msg()
    }

    /// Returns the user data associated with the task.
    pub fn user_data(&self) -> &T {
        &self.inner.get().user_data
    }

    /// Returns the user data associated with the task.
    pub fn user_data_mut(&mut self) -> &mut T {
        &mut self.inner.get_mut().user_data
    }

    /// Returns the task id.
    pub fn id(&self) -> TaskId {
        *self.inner.key()
    }

    /// Closes the task. Returns the user data.
    ///
    /// No further event will be generated for this task, but the connection inside the task will
    /// continue to run until the `ClosedTask` is destroyed.
    pub fn close(self) -> ClosedTask<E, T> {
        let id = *self.inner.key();
        let task = self.inner.remove();
        ClosedTask::new(id, task.sender, task.user_data)
    }

    /// Gives ownership of a closed task.
    /// As soon as our task (`self`) has some acknowledgment from the remote
    /// that its connection is alive, it will close the connection with `other`.
    ///
    /// Make sure to complete this operation with `complete_take_over`.
    #[must_use]
    pub fn start_take_over(&mut self, t: ClosedTask<E, T>) -> StartTakeOver<T, ClosedTask<E, T>> {
        // It is possible that the sender is closed if the background task has already finished
        // but the local state hasn't been updated yet because we haven't been polled in the
        // meanwhile.
        let id = t.id();
        match self.start_send_event_msg(ToTaskMessage::TakeOver(t.sender)) {
            Ok(AsyncSink::Ready) => StartTakeOver::Ready(t.user_data),
            Ok(AsyncSink::NotReady(ToTaskMessage::TakeOver(sender))) =>
                StartTakeOver::NotReady(ClosedTask::new(id, sender, t.user_data)),
            Ok(AsyncSink::NotReady(_)) =>
                unreachable!("We tried to send a take over message, so we get one back."),
            Err(()) => StartTakeOver::Gone
        }
    }

    /// Finish take over started by `start_take_over`.
    pub fn complete_take_over(&mut self) -> Poll<(), ()> {
        self.complete_send_event_msg()
    }

    /// Begin to send a message to the task.
    ///
    /// The API mimicks the one of [`futures::Sink`]. If this method returns
    /// `Ok(AsyncSink::Ready)` drive the sending to completion with
    /// `complete_send_event_msg`. If the receiving end does not longer exist,
    /// i.e. the task has ended, we return this information as an error.
    fn start_send_event_msg(&mut self, msg: ToTaskMessage<E>) -> StartSend<ToTaskMessage<E>, ()> {
        // We first drive any pending send to completion before starting another one.
        if self.complete_send_event_msg()?.is_ready() {
            self.inner.get_mut().pending = Some(AsyncSink::NotReady(msg));
            Ok(AsyncSink::Ready)
        } else {
            Ok(AsyncSink::NotReady(msg))
        }
    }

    /// Complete event message deliver started by `start_send_event_msg`.
    fn complete_send_event_msg(&mut self) -> Poll<(), ()> {
        // It is possible that the sender is closed if the background task has already finished
        // but the local state hasn't been updated yet because we haven't been polled in the
        // meanwhile.
        let task = self.inner.get_mut();
        let state =
            if let Some(state) = task.pending.take() {
                state
            } else {
                return Ok(Async::Ready(()))
            };
        match state {
            AsyncSink::NotReady(msg) =>
                match task.sender.start_send(msg).map_err(|_| ())? {
                    AsyncSink::Ready =>
                        if task.sender.poll_complete().map_err(|_| ())?.is_not_ready() {
                            task.pending = Some(AsyncSink::Ready);
                            Ok(Async::NotReady)
                        } else {
                            Ok(Async::Ready(()))
                        }
                    AsyncSink::NotReady(msg) => {
                        task.pending = Some(AsyncSink::NotReady(msg));
                        Ok(Async::NotReady)
                    }
                }
            AsyncSink::Ready =>
                if task.sender.poll_complete().map_err(|_| ())?.is_not_ready() {
                    task.pending = Some(AsyncSink::Ready);
                    Ok(Async::NotReady)
                } else {
                    Ok(Async::Ready(()))
                }
        }
    }
}

impl<E, T: fmt::Debug> fmt::Debug for TaskEntry<'_, E, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("TaskEntry")
            .field(&self.id())
            .field(self.user_data())
            .finish()
    }
}

/// Result of [`TaskEntry::start_take_over`].
#[derive(Debug)]
pub enum StartTakeOver<A, B> {
    /// The take over message has been enqueued.
    /// Complete the take over with [`TaskEntry::complete_take_over`].
    Ready(A),
    /// Not ready to send the take over message to the task.
    NotReady(B),
    /// The task to send the take over message is no longer there.
    Gone
}

/// Task after it has been closed.
///
/// The connection to the remote is potentially still going on, but no new
/// event for this task will be received.
pub struct ClosedTask<E, T> {
    /// Identifier of the task that closed.
    ///
    /// No longer corresponds to anything, but can be reported to the user.
    id: TaskId,

    /// The channel to the task.
    ///
    /// The task will continue to work for as long as this channel is alive,
    /// but events produced by it are ignored.
    sender: mpsc::Sender<ToTaskMessage<E>>,

    /// The data provided by the user.
    user_data: T
}

impl<E, T> ClosedTask<E, T> {
    /// Create a new `ClosedTask` value.
    fn new(id: TaskId, sender: mpsc::Sender<ToTaskMessage<E>>, user_data: T) -> Self {
        Self { id, sender, user_data }
    }

    /// Returns the task id.
    ///
    /// Note that this task is no longer managed and therefore calling
    /// `Manager::task()` with this ID will fail.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the user data associated with the task.
    pub fn user_data(&self) -> &T {
        &self.user_data
    }

    /// Returns the user data associated with the task.
    pub fn user_data_mut(&mut self) -> &mut T {
        &mut self.user_data
    }

    /// Finish destroying the task and yield the user data.
    /// This closes the connection to the remote.
    pub fn into_user_data(self) -> T {
        self.user_data
    }
}

impl<E, T: fmt::Debug> fmt::Debug for ClosedTask<E, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("ClosedTask")
            .field(&self.id)
            .field(&self.user_data)
            .finish()
    }
}

