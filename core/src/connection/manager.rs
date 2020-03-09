// Copyright 2020 Parity Technologies (UK) Ltd.
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
    Executor,
    muxing::StreamMuxer,
};
use fnv::FnvHashMap;
use futures::{
    prelude::*,
    channel::mpsc,
    stream::FuturesUnordered
};
use std::{
    collections::hash_map,
    error,
    fmt,
    pin::Pin,
    task::{Context, Poll},
};
use super::{
    Connected,
    Connection,
    ConnectionError,
    ConnectionHandler,
    IntoConnectionHandler,
    PendingConnectionError,
    Substream
};
use task::{Task, TaskId};

mod task;

// Implementation Notes
// ====================
//
// A `Manager` is decoupled from the background tasks through channels.
// The state of a `Manager` therefore "lags behind" the progress of
// the tasks -- it is only made aware of progress in the background tasks
// when it is `poll()`ed.
//
// A `Manager` is ignorant of substreams and does not emit any events
// related to specific substreams.
//
// A `Manager` is unaware of any association between connections and peers
// / peer identities (i.e. the type parameter `C` is completely opaque).
//
// There is a 1-1 correspondence between (internal) task IDs and (public)
// connection IDs, i.e. the task IDs are "re-exported" as connection IDs
// by the manager. The notion of a (background) task is internal to the
// manager.

/// The result of a pending connection attempt.
type ConnectResult<C, M, TE> = Result<(Connected<C>, M), PendingConnectionError<TE>>;

/// Connection identifier.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionId(TaskId);

impl ConnectionId {
    /// Creates a `ConnectionId` from a non-negative integer.
    ///
    /// This is primarily useful for creating connection IDs
    /// in test environments. There is in general no guarantee
    /// that all connection IDs are based on non-negative integers.
    pub fn new(id: usize) -> Self {
        ConnectionId(TaskId(id))
    }
}

/// A connection `Manager` orchestrates the I/O of a set of connections.
pub struct Manager<I, O, H, E, HE, C> {
    /// The tasks of the managed connections.
    ///
    /// Each managed connection is associated with a (background) task
    /// spawned onto an executor. Each `TaskInfo` in `tasks` is linked to such a
    /// background task via a channel. Closing that channel (i.e. dropping
    /// the sender in the associated `TaskInfo`) stops the background task,
    /// which will attempt to gracefully close the connection.
    tasks: FnvHashMap<TaskId, TaskInfo<I, C>>,

    /// Next available identifier for a new connection / task.
    next_task_id: TaskId,

    /// The executor to use for running the background tasks. If `None`,
    /// the tasks are kept in `local_spawns` instead and polled on the
    /// current thread when the manager is polled for new events.
    executor: Option<Box<dyn Executor + Send>>,

    /// If no `executor` is configured, tasks are kept in this set and
    /// polled on the current thread when the manager is polled for new events.
    local_spawns: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Send>>>,

    /// Sender distributed to managed tasks for reporting events back
    /// to the manager.
    events_tx: mpsc::Sender<task::Event<O, H, E, HE, C>>,

    /// Receiver for events reported from managed tasks.
    events_rx: mpsc::Receiver<task::Event<O, H, E, HE, C>>
}

impl<I, O, H, E, HE, C> fmt::Debug for Manager<I, O, H, E, HE, C>
where
    C: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_map()
            .entries(self.tasks.iter().map(|(id, task)| (id, &task.state)))
            .finish()
    }
}

/// Internal information about a running task.
///
/// Contains the sender to deliver event messages to the task, and
/// the associated user data.
#[derive(Debug)]
struct TaskInfo<I, C> {
    /// channel endpoint to send messages to the task
    sender: mpsc::Sender<task::Command<I>>,
    /// The state of the task as seen by the `Manager`.
    state: TaskState<C>,
}

/// Internal state of a running task as seen by the `Manager`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState<C> {
    /// The connection is being established.
    Pending,
    /// The connection is established.
    Established(Connected<C>),
}

/// Events produced by the [`Manager`].
#[derive(Debug)]
pub enum Event<'a, I, O, H, TE, HE, C> {
    /// A connection attempt has failed.
    PendingConnectionError {
        /// The connection ID.
        ///
        /// As a result of the error, the pending connection has been removed
        /// from the `Manager` and is being closed. Hence this ID will
        /// no longer resolve to a valid entry in the manager.
        id: ConnectionId,
        /// What happened.
        error: PendingConnectionError<TE>,
        /// The handler that was supposed to handle the failed connection.
        handler: H
    },

    /// An established connection has encountered an error.
    ConnectionError {
        /// The connection ID.
        ///
        /// As a result of the error, the connection has been removed
        /// from the `Manager` and is being closed. Hence this ID will
        /// no longer resolve to a valid entry in the manager.
        id: ConnectionId,
        /// Information about the connection that encountered the error.
        connected: Connected<C>,
        /// The error that occurred.
        error: ConnectionError<HE>,
    },

    /// A connection has been established.
    ConnectionEstablished {
        /// The entry associated with the new connection.
        entry: EstablishedEntry<'a, I, C>,
    },

    /// A connection handler has produced an event.
    ConnectionEvent {
        /// The entry associated with the connection that produced the event.
        entry: EstablishedEntry<'a, I, C>,
        /// The produced event.
        event: O
    }
}

impl<I, O, H, TE, HE, C> Manager<I, O, H, TE, HE, C> {
    /// Creates a new connection manager.
    pub fn new(executor: Option<Box<dyn Executor + Send>>) -> Self {
        let (tx, rx) = mpsc::channel(1);
        Self {
            tasks: FnvHashMap::default(),
            next_task_id: TaskId(0),
            executor,
            local_spawns: FuturesUnordered::new(),
            events_tx: tx,
            events_rx: rx
        }
    }

    /// Adds to the manager a future that tries to reach a node.
    ///
    /// This method spawns a task dedicated to resolving this future and
    /// processing the node's events.
    pub fn add_pending<F, M>(&mut self, future: F, handler: H) -> ConnectionId
    where
        I: Send + 'static,
        O: Send + 'static,
        TE: error::Error + Send + 'static,
        HE: error::Error + Send + 'static,
        C: Send + 'static,
        M: StreamMuxer + Send + Sync + 'static,
        M::OutboundSubstream: Send + 'static,
        F: Future<Output = ConnectResult<C, M, TE>> + Send + 'static,
        H: IntoConnectionHandler<C> + Send + 'static,
        H::Handler: ConnectionHandler<
            Substream = Substream<M>,
            InEvent = I,
            OutEvent = O,
            Error = HE
        > + Send + 'static,
        <H::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::channel(4);
        self.tasks.insert(task_id, TaskInfo { sender: tx, state: TaskState::Pending });

        let task = Box::pin(Task::pending(task_id, self.events_tx.clone(), rx, future, handler));
        if let Some(executor) = &mut self.executor {
            executor.exec(task);
        } else {
            self.local_spawns.push(task);
        }

        ConnectionId(task_id)
    }

    /// Adds an existing connection to the manager.
    pub fn add<M>(&mut self, conn: Connection<M, H::Handler>, info: Connected<C>) -> ConnectionId
    where
        H: IntoConnectionHandler<C> + Send + 'static,
        H::Handler: ConnectionHandler<
            Substream = Substream<M>,
            InEvent = I,
            OutEvent = O,
            Error = HE
        > + Send + 'static,
        <H::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TE: error::Error + Send + 'static,
        HE: error::Error + Send + 'static,
        I: Send + 'static,
        O: Send + 'static,
        M: StreamMuxer + Send + Sync + 'static,
        M::OutboundSubstream: Send + 'static,
        C: Send + 'static
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::channel(4);
        self.tasks.insert(task_id, TaskInfo {
            sender: tx, state: TaskState::Established(info)
        });

        let task: Pin<Box<Task<Pin<Box<future::Pending<_>>>, _, _, _, _, _, _>>> =
            Box::pin(Task::established(task_id, self.events_tx.clone(), rx, conn));

        if let Some(executor) = &mut self.executor {
            executor.exec(task);
        } else {
            self.local_spawns.push(task);
        }

        ConnectionId(task_id)
    }

    /// Notifies the handlers of all managed connections of an event.
    ///
    /// This function is "atomic", in the sense that if `Poll::Pending` is
    /// returned then no event has been sent.
    #[must_use]
    pub fn poll_broadcast(&mut self, event: &I, cx: &mut Context) -> Poll<()>
    where
        I: Clone
    {
        for task in self.tasks.values_mut() {
            if let Poll::Pending = task.sender.poll_ready(cx) { // (*)
                return Poll::Pending;
            }
        }

        for (id, task) in self.tasks.iter_mut() {
            let cmd = task::Command::NotifyHandler(event.clone());
            match task.sender.start_send(cmd) {
                Ok(()) => {},
                Err(e) if e.is_full() => unreachable!("by (*)"),
                Err(e) if e.is_disconnected() => {
                    // The background task ended. The manager will eventually be
                    // informed through an `Error` event from the task.
                    log::trace!("Connection dropped: {:?}", id);
                },
                Err(e) => {
                    log::error!("Unexpected error: {:?}", e);
                }
            }
        }

        Poll::Ready(())
    }

    /// Gets an entry for a managed connection, if it exists.
    pub fn entry(&mut self, id: ConnectionId) -> Option<Entry<'_, I, C>> {
        if let hash_map::Entry::Occupied(task) = self.tasks.entry(id.0) {
            Some(Entry::new(task))
        } else {
            None
        }
    }

    /// Checks whether an established connection with the given ID is currently managed.
    pub fn is_established(&self, id: &ConnectionId) -> bool {
        match self.tasks.get(&id.0) {
            Some(TaskInfo { state: TaskState::Established(..), .. }) => true,
            _ => false
        }
    }

    /// Polls the manager for events relating to the managed connections.
    pub fn poll<'a>(&'a mut self, cx: &mut Context) -> Poll<Event<'a, I, O, H, TE, HE, C>> {
        // Advance the content of `local_spawns`.
        while let Poll::Ready(Some(_)) = Stream::poll_next(Pin::new(&mut self.local_spawns), cx) {}

        // Poll for the first event for which the manager still has a registered task, if any.
        let event = loop {
            match Stream::poll_next(Pin::new(&mut self.events_rx), cx) {
                Poll::Ready(Some(event)) => {
                    if self.tasks.contains_key(event.id()) { // (1)
                        break event
                    }
                }
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => unreachable!("Manager holds both sender and receiver."),
            }
        };

        if let hash_map::Entry::Occupied(mut task) = self.tasks.entry(*event.id()) {
            Poll::Ready(match event {
                task::Event::Notify { id: _, event } =>
                    Event::ConnectionEvent {
                        entry: EstablishedEntry { task },
                        event
                    },
                task::Event::Established { id: _, info } => { // (2)
                    task.get_mut().state = TaskState::Established(info); // (3)
                    Event::ConnectionEstablished {
                        entry: EstablishedEntry { task },
                    }
                }
                task::Event::Failed { id, error, handler } => {
                    let id = ConnectionId(id);
                    let _ = task.remove();
                    Event::PendingConnectionError { id, error, handler }
                }
                task::Event::Error { id, error } => {
                    let id = ConnectionId(id);
                    let task = task.remove();
                    match task.state {
                        TaskState::Established(connected) =>
                            Event::ConnectionError { id, connected, error },
                        TaskState::Pending => unreachable!(
                            "`Event::Error` implies (2) occurred on that task and thus (3)."
                            ),
                    }
                }

            })
        } else {
            unreachable!("By (1)")
        }
    }
}

/// An entry for a connection in the manager.
#[derive(Debug)]
pub enum Entry<'a, I, C> {
    Pending(PendingEntry<'a, I, C>),
    Established(EstablishedEntry<'a, I, C>)
}

impl<'a, I, C> Entry<'a, I, C> {
    fn new(task: hash_map::OccupiedEntry<'a, TaskId, TaskInfo<I, C>>) -> Self {
        match &task.get().state {
            TaskState::Pending => Entry::Pending(PendingEntry { task }),
            TaskState::Established(_) => Entry::Established(EstablishedEntry { task })
        }
    }
}

/// An entry for a managed connection that is considered established.
#[derive(Debug)]
pub struct EstablishedEntry<'a, I, C> {
    task: hash_map::OccupiedEntry<'a, TaskId, TaskInfo<I, C>>,
}

impl<'a, I, C> EstablishedEntry<'a, I, C> {
    /// (Asynchronously) sends an event to the connection handler.
    ///
    /// If the handler is not ready to receive the event, either because
    /// it is busy or the connection is about to close, the given event
    /// is returned with an `Err`.
    ///
    /// If execution of this method is preceded by successful execution of
    /// `poll_ready_notify_handler` without another intervening execution
    /// of `notify_handler`, it only fails if the connection is now about
    /// to close.
    ///
    /// > **Note**: As this method does not take a `Context`, the current
    /// > task _may not be notified_ if sending the event fails due to
    /// > the connection handler not being ready at this time.
    pub fn notify_handler(&mut self, event: I) -> Result<(), I> {
        let cmd = task::Command::NotifyHandler(event);
        self.task.get_mut().sender.try_send(cmd)
            .map_err(|e| match e.into_inner() {
                task::Command::NotifyHandler(event) => event
            })
    }

    /// Checks if `notify_handler` is ready to accept an event.
    ///
    /// Returns `Ok(())` if the handler is ready to receive an event via `notify_handler`.
    ///
    /// Returns `Err(())` if the background task associated with the connection
    /// is terminating and the connection is about to close.
    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context) -> Poll<Result<(),()>> {
        self.task.get_mut().sender.poll_ready(cx).map_err(|_| ())
    }

    /// Obtains information about the established connection.
    pub fn connected(&self) -> &Connected<C> {
        match &self.task.get().state {
            TaskState::Established(c) => c,
            TaskState::Pending => unreachable!("By Entry::new()")
        }
    }

    /// Closes the connection represented by this entry,
    /// returning the connection information.
    pub fn close(self) -> Connected<C> {
        match self.task.remove().state {
            TaskState::Established(c) => c,
            TaskState::Pending => unreachable!("By Entry::new()")
        }
    }

    /// Returns the connection id.
    pub fn id(&self) -> ConnectionId {
        ConnectionId(*self.task.key())
    }
}

/// An entry for a managed connection that is currently being established
/// (i.e. pending).
#[derive(Debug)]
pub struct PendingEntry<'a, I, C> {
    task: hash_map::OccupiedEntry<'a, TaskId, TaskInfo<I, C>>
}

impl<'a, I, C> PendingEntry<'a, I, C> {
    /// Returns the connection id.
    pub fn id(&self) -> ConnectionId {
        ConnectionId(*self.task.key())
    }

    /// Aborts the pending connection attempt.
    pub fn abort(self)  {
        self.task.remove();
    }
}
