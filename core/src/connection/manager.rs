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

use super::{
    handler::{THandlerError, THandlerInEvent, THandlerOutEvent},
    Connected, ConnectedPoint, Connection, ConnectionError, ConnectionHandler,
    IntoConnectionHandler, PendingConnectionError, Substream,
};
use crate::{muxing::StreamMuxer, Executor};
use fnv::FnvHashMap;
use futures::{channel::mpsc, prelude::*, stream::FuturesUnordered};
use std::{
    collections::hash_map,
    error, fmt, mem,
    pin::Pin,
    task::{Context, Poll},
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
type ConnectResult<M, TE> = Result<(Connected, M), PendingConnectionError<TE>>;

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
pub struct Manager<H: IntoConnectionHandler, E> {
    /// The tasks of the managed connections.
    ///
    /// Each managed connection is associated with a (background) task
    /// spawned onto an executor. Each `TaskInfo` in `tasks` is linked to such a
    /// background task via a channel. Closing that channel (i.e. dropping
    /// the sender in the associated `TaskInfo`) stops the background task,
    /// which will attempt to gracefully close the connection.
    tasks: FnvHashMap<TaskId, TaskInfo<THandlerInEvent<H>>>,

    /// Next available identifier for a new connection / task.
    next_task_id: TaskId,

    /// Size of the task command buffer (per task).
    task_command_buffer_size: usize,

    /// The executor to use for running the background tasks. If `None`,
    /// the tasks are kept in `local_spawns` instead and polled on the
    /// current thread when the manager is polled for new events.
    executor: Option<Box<dyn Executor + Send>>,

    /// If no `executor` is configured, tasks are kept in this set and
    /// polled on the current thread when the manager is polled for new events.
    local_spawns: FuturesUnordered<Pin<Box<dyn Future<Output = ()> + Send>>>,

    /// Sender distributed to managed tasks for reporting events back
    /// to the manager.
    events_tx: mpsc::Sender<task::Event<H, E>>,

    /// Receiver for events reported from managed tasks.
    events_rx: mpsc::Receiver<task::Event<H, E>>,
}

impl<H: IntoConnectionHandler, E> fmt::Debug for Manager<H, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.tasks.iter().map(|(id, task)| (id, &task.state)))
            .finish()
    }
}

/// Configuration options when creating a [`Manager`].
///
/// The default configuration specifies no dedicated task executor, a
/// task event buffer size of 32, and a task command buffer size of 7.
#[non_exhaustive]
pub struct ManagerConfig {
    /// Executor to use to spawn tasks.
    pub executor: Option<Box<dyn Executor + Send>>,

    /// Size of the task command buffer (per task).
    pub task_command_buffer_size: usize,

    /// Size of the task event buffer (for all tasks).
    pub task_event_buffer_size: usize,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        ManagerConfig {
            executor: None,
            task_event_buffer_size: 32,
            task_command_buffer_size: 7,
        }
    }
}

/// Internal information about a running task.
///
/// Contains the sender to deliver event messages to the task, and
/// the associated user data.
#[derive(Debug)]
struct TaskInfo<I> {
    /// Channel endpoint to send messages to the task.
    sender: mpsc::Sender<task::Command<I>>,
    /// The state of the task as seen by the `Manager`.
    state: TaskState,
}

/// Internal state of a running task as seen by the `Manager`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskState {
    /// The connection is being established.
    Pending,
    /// The connection is established.
    Established(Connected),
}

/// Events produced by the [`Manager`].
#[derive(Debug)]
pub enum Event<'a, H: IntoConnectionHandler, TE> {
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
        handler: H,
    },

    /// An established connection has been closed.
    ConnectionClosed {
        /// The connection ID.
        ///
        /// > **Note**: Closed connections are removed from the `Manager`.
        /// > Hence this ID will no longer resolve to a valid entry in
        /// > the manager.
        id: ConnectionId,
        /// Information about the closed connection.
        connected: Connected,
        /// The error that occurred, if any. If `None`, the connection
        /// has been actively closed.
        error: Option<ConnectionError<THandlerError<H>>>,
    },

    /// A connection has been established.
    ConnectionEstablished {
        /// The entry associated with the new connection.
        entry: EstablishedEntry<'a, THandlerInEvent<H>>,
    },

    /// A connection handler has produced an event.
    ConnectionEvent {
        /// The entry associated with the connection that produced the event.
        entry: EstablishedEntry<'a, THandlerInEvent<H>>,
        /// The produced event.
        event: THandlerOutEvent<H>,
    },

    /// A connection to a node has changed its address.
    AddressChange {
        /// The entry associated with the connection that changed address.
        entry: EstablishedEntry<'a, THandlerInEvent<H>>,
        /// The former [`ConnectedPoint`].
        old_endpoint: ConnectedPoint,
        /// The new [`ConnectedPoint`].
        new_endpoint: ConnectedPoint,
    },
}

impl<H: IntoConnectionHandler, TE> Manager<H, TE> {
    /// Creates a new connection manager.
    pub fn new(config: ManagerConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.task_event_buffer_size);
        Self {
            tasks: FnvHashMap::default(),
            next_task_id: TaskId(0),
            task_command_buffer_size: config.task_command_buffer_size,
            executor: config.executor,
            local_spawns: FuturesUnordered::new(),
            events_tx: tx,
            events_rx: rx,
        }
    }

    /// Adds to the manager a future that tries to reach a node.
    ///
    /// This method spawns a task dedicated to resolving this future and
    /// processing the node's events.
    pub fn add_pending<F, M>(&mut self, future: F, handler: H) -> ConnectionId
    where
        TE: error::Error + Send + 'static,
        M: StreamMuxer + Send + Sync + 'static,
        M::OutboundSubstream: Send + 'static,
        F: Future<Output = ConnectResult<M, TE>> + Send + 'static,
        H: IntoConnectionHandler + Send + 'static,
        H::Handler: ConnectionHandler<Substream = Substream<M>> + Send + 'static,
        <H::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::channel(self.task_command_buffer_size);
        self.tasks.insert(
            task_id,
            TaskInfo {
                sender: tx,
                state: TaskState::Pending,
            },
        );

        let task = Box::pin(Task::pending(
            task_id,
            self.events_tx.clone(),
            rx,
            future,
            handler,
        ));
        if let Some(executor) = &mut self.executor {
            executor.exec(task);
        } else {
            self.local_spawns.push(task);
        }

        ConnectionId(task_id)
    }

    /// Adds an existing connection to the manager.
    pub fn add<M>(&mut self, conn: Connection<M, H::Handler>, info: Connected) -> ConnectionId
    where
        H: IntoConnectionHandler + Send + 'static,
        H::Handler: ConnectionHandler<Substream = Substream<M>> + Send + 'static,
        <H::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
        TE: error::Error + Send + 'static,
        M: StreamMuxer + Send + Sync + 'static,
        M::OutboundSubstream: Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::channel(self.task_command_buffer_size);
        self.tasks.insert(
            task_id,
            TaskInfo {
                sender: tx,
                state: TaskState::Established(info),
            },
        );

        let task: Pin<Box<Task<Pin<Box<future::Pending<_>>>, _, _, _>>> =
            Box::pin(Task::established(task_id, self.events_tx.clone(), rx, conn));

        if let Some(executor) = &mut self.executor {
            executor.exec(task);
        } else {
            self.local_spawns.push(task);
        }

        ConnectionId(task_id)
    }

    /// Gets an entry for a managed connection, if it exists.
    pub fn entry(&mut self, id: ConnectionId) -> Option<Entry<'_, THandlerInEvent<H>>> {
        if let hash_map::Entry::Occupied(task) = self.tasks.entry(id.0) {
            Some(Entry::new(task))
        } else {
            None
        }
    }

    /// Checks whether an established connection with the given ID is currently managed.
    pub fn is_established(&self, id: &ConnectionId) -> bool {
        matches!(
            self.tasks.get(&id.0),
            Some(TaskInfo {
                state: TaskState::Established(..),
                ..
            })
        )
    }

    /// Polls the manager for events relating to the managed connections.
    pub fn poll<'a>(&'a mut self, cx: &mut Context<'_>) -> Poll<Event<'a, H, TE>> {
        // Advance the content of `local_spawns`.
        while let Poll::Ready(Some(_)) = self.local_spawns.poll_next_unpin(cx) {}

        // Poll for the first event for which the manager still has a registered task, if any.
        let event = loop {
            match self.events_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => {
                    if self.tasks.contains_key(event.id()) {
                        // (1)
                        break event;
                    }
                }
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => unreachable!("Manager holds both sender and receiver."),
            }
        };

        if let hash_map::Entry::Occupied(mut task) = self.tasks.entry(*event.id()) {
            Poll::Ready(match event {
                task::Event::Notify { id: _, event } => Event::ConnectionEvent {
                    entry: EstablishedEntry { task },
                    event,
                },
                task::Event::Established { id: _, info } => {
                    // (2)
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
                task::Event::AddressChange { id: _, new_address } => {
                    let (new, old) = if let TaskState::Established(c) = &mut task.get_mut().state {
                        let mut new_endpoint = c.endpoint.clone();
                        new_endpoint.set_remote_address(new_address);
                        let old_endpoint = mem::replace(&mut c.endpoint, new_endpoint.clone());
                        (new_endpoint, old_endpoint)
                    } else {
                        unreachable!(
                            "`Event::AddressChange` implies (2) occurred on that task and thus (3)."
                        )
                    };
                    Event::AddressChange {
                        entry: EstablishedEntry { task },
                        old_endpoint: old,
                        new_endpoint: new,
                    }
                }
                task::Event::Closed { id, error } => {
                    let id = ConnectionId(id);
                    let task = task.remove();
                    match task.state {
                        TaskState::Established(connected) => Event::ConnectionClosed {
                            id,
                            connected,
                            error,
                        },
                        TaskState::Pending => unreachable!(
                            "`Event::Closed` implies (2) occurred on that task and thus (3)."
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
pub enum Entry<'a, I> {
    Pending(PendingEntry<'a, I>),
    Established(EstablishedEntry<'a, I>),
}

impl<'a, I> Entry<'a, I> {
    fn new(task: hash_map::OccupiedEntry<'a, TaskId, TaskInfo<I>>) -> Self {
        match &task.get().state {
            TaskState::Pending => Entry::Pending(PendingEntry { task }),
            TaskState::Established(_) => Entry::Established(EstablishedEntry { task }),
        }
    }
}

/// An entry for a managed connection that is considered established.
#[derive(Debug)]
pub struct EstablishedEntry<'a, I> {
    task: hash_map::OccupiedEntry<'a, TaskId, TaskInfo<I>>,
}

impl<'a, I> EstablishedEntry<'a, I> {
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
        let cmd = task::Command::NotifyHandler(event); // (*)
        self.task
            .get_mut()
            .sender
            .try_send(cmd)
            .map_err(|e| match e.into_inner() {
                task::Command::NotifyHandler(event) => event,
                _ => panic!("Unexpected command. Expected `NotifyHandler`"), // see (*)
            })
    }

    /// Checks if `notify_handler` is ready to accept an event.
    ///
    /// Returns `Ok(())` if the handler is ready to receive an event via `notify_handler`.
    ///
    /// Returns `Err(())` if the background task associated with the connection
    /// is terminating and the connection is about to close.
    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        self.task.get_mut().sender.poll_ready(cx).map_err(|_| ())
    }

    /// Sends a close command to the associated background task,
    /// thus initiating a graceful active close of the connection.
    ///
    /// Has no effect if the connection is already closing.
    ///
    /// When the connection is ultimately closed, [`Event::ConnectionClosed`]
    /// is emitted by [`Manager::poll`].
    pub fn start_close(mut self) {
        // Clone the sender so that we are guaranteed to have
        // capacity for the close command (every sender gets a slot).
        match self
            .task
            .get_mut()
            .sender
            .clone()
            .try_send(task::Command::Close)
        {
            Ok(()) => {}
            Err(e) => assert!(e.is_disconnected(), "No capacity for close command."),
        }
    }

    /// Obtains information about the established connection.
    pub fn connected(&self) -> &Connected {
        match &self.task.get().state {
            TaskState::Established(c) => c,
            TaskState::Pending => unreachable!("By Entry::new()"),
        }
    }

    /// Instantly removes the entry from the manager, dropping
    /// the command channel to the background task of the connection,
    /// which will thus drop the connection asap without an orderly
    /// close or emitting another event.
    pub fn remove(self) -> Connected {
        match self.task.remove().state {
            TaskState::Established(c) => c,
            TaskState::Pending => unreachable!("By Entry::new()"),
        }
    }

    /// Returns the connection ID.
    pub fn id(&self) -> ConnectionId {
        ConnectionId(*self.task.key())
    }
}

/// An entry for a managed connection that is currently being established
/// (i.e. pending).
#[derive(Debug)]
pub struct PendingEntry<'a, I> {
    task: hash_map::OccupiedEntry<'a, TaskId, TaskInfo<I>>,
}

impl<'a, I> PendingEntry<'a, I> {
    /// Returns the connection id.
    pub fn id(&self) -> ConnectionId {
        ConnectionId(*self.task.key())
    }

    /// Aborts the pending connection attempt.
    pub fn abort(self) {
        self.task.remove();
    }
}
