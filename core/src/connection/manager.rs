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
    Connected, ConnectedPoint, ConnectionError, ConnectionHandler, IntoConnectionHandler,
    PendingConnectionError, Substream,
};
use crate::{muxing::StreamMuxer, transport::TransportError, Executor, Multiaddr, PeerId};
use fnv::FnvHashMap;
use futures::{
    channel::{mpsc, oneshot},
    future::{poll_fn, BoxFuture, Either},
    prelude::*,
    ready,
    stream::FuturesUnordered,
};
use std::{
    collections::hash_map,
    error, fmt, mem,
    pin::Pin,
    task::{Context, Poll},
};
use task::{Task, TaskId};
use void::Void;

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
pub struct Manager<H: IntoConnectionHandler, TMuxer, E> {
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
    events_tx: mpsc::Sender<task::Event<H, TMuxer, E>>,

    /// Receiver for events reported from managed tasks.
    events_rx: mpsc::Receiver<task::Event<H, TMuxer, E>>,
}

impl<H: IntoConnectionHandler, TMuxer, E> fmt::Debug for Manager<H, TMuxer, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.tasks.iter()).finish()
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
enum TaskInfo<I> {
    Pending {
        /// When dropped, notifies the task which can then terminate.
        drop_notifier: oneshot::Sender<Void>,
    },
    Established {
        connected: Connected,
        /// Channel endpoint to send messages to the task.
        sender: mpsc::Sender<task::Command<I>>,
    },
}

/// Events produced by the [`Manager`].
#[derive(Debug)]
pub enum Event<'a, H: IntoConnectionHandler, TMuxer, TE> {
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
        handler: H::Handler,
    },

    /// A connection has been established.
    OutgoingConnectionEstablished {
        id: ConnectionId,
        peer_id: PeerId,
        address: Multiaddr,
        muxer: TMuxer,
        errors: Vec<(Multiaddr, TransportError<TE>)>,
    },

    /// A connection has been established.
    IncomingConnectionEstablished {
        id: ConnectionId,
        peer_id: PeerId,
        muxer: TMuxer,
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

impl<H: IntoConnectionHandler, TMuxer: Send + 'static, TE: Send + 'static> Manager<H, TMuxer, TE> {
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

    fn next_task_id(&mut self) -> TaskId {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        task_id
    }

    fn spawn(&mut self, task: BoxFuture<'static, ()>) {
        if let Some(executor) = &mut self.executor {
            executor.exec(task);
        } else {
            self.local_spawns.push(task);
        }
    }

    pub fn add_closing(&mut self, muxer: TMuxer)
    where
        TMuxer: StreamMuxer + Send + Sync + 'static,
    {
        self.spawn(
            poll_fn(move |cx| {
                ready!(muxer.close(cx));
                // TODO: Should we send the result back to the manager?
                Poll::Ready(())
            })
            .boxed(),
        );
    }

    pub fn add_pending_incoming<TFut>(&mut self, dial: TFut) -> ConnectionId
    where
        TFut:
            Future<Output = Result<(PeerId, TMuxer), PendingConnectionError<TE>>> + Send + 'static,

        H: IntoConnectionHandler + Send + 'static,
        H::Handler: ConnectionHandler + Send + 'static,
    {
        let task_id = self.next_task_id();

        let (drop_notifier, mut drop_receiver) = oneshot::channel();
        self.tasks
            .insert(task_id, TaskInfo::Pending { drop_notifier });

        let mut events = self.events_tx.clone();

        self.spawn(
            async move {
                match futures::future::select(drop_receiver, Box::pin(dial)).await {
                    Either::Left((Err(oneshot::Canceled), _)) => {
                        events
                            .send(task::Event::PendingFailed {
                                id: task_id,
                                error: PendingConnectionError::Aborted,
                            })
                            .await;
                    }
                    Either::Left((Ok(v), _)) => void::unreachable(v),
                    Either::Right((Ok((peer_id, muxer)), _)) => {
                        events
                            .send(task::Event::IncomingEstablished {
                                id: task_id,
                                peer_id,
                                muxer,
                            })
                            .await;
                    }
                    Either::Right((Err(e), _)) => {
                        events
                            .send(task::Event::PendingFailed {
                                id: task_id,
                                error: e,
                            })
                            .await;
                    }
                }
            }
            .boxed(),
        );

        ConnectionId(task_id)
    }

    pub fn add_pending_outgoing(
        &mut self,
        dial: crate::network::concurrent_dial::ConcurrentDial<TMuxer, TE>,
    ) -> ConnectionId
    where
        H: IntoConnectionHandler + Send + 'static,
        H::Handler: ConnectionHandler + Send + 'static,
    {
        let task_id = self.next_task_id();

        let (drop_notifier, mut drop_receiver) = oneshot::channel();

        self.tasks
            .insert(task_id, TaskInfo::Pending { drop_notifier });

        let mut events = self.events_tx.clone();

        self.spawn(
            async move {
                match futures::future::select(drop_receiver, Box::pin(dial)).await {
                    Either::Left((Err(oneshot::Canceled), _)) => {
                        events
                            .send(task::Event::PendingFailed {
                                id: task_id,
                                error: PendingConnectionError::Aborted,
                            })
                            .await;
                    }
                    Either::Left((Ok(v), _)) => void::unreachable(v),
                    Either::Right((Ok((peer_id, address, muxer, errors)), _)) => {
                        events
                            .send(task::Event::OutgoingEstablished {
                                id: task_id,
                                peer_id,
                                muxer,
                                address,
                                errors,
                            })
                            .await;
                    }
                    Either::Right((Err(e), _)) => {
                        events
                            .send(task::Event::PendingFailed {
                                id: task_id,
                                error: PendingConnectionError::TransportDial(e),
                            })
                            .await;
                    }
                }
            }
            .boxed(),
        );

        ConnectionId(task_id)
    }

    pub fn add_established(
        &mut self,
        connection_id: ConnectionId,
        mut connection: crate::connection::Connection<TMuxer, H::Handler>,
        connected: Connected,
    ) where
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send + 'static,
        H: IntoConnectionHandler + Send + 'static,
        H::Handler: ConnectionHandler<Substream = Substream<TMuxer>> + Send + 'static,
        <H::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
    {
        let task_id = connection_id.0;
        let (command_sender, mut command_receiver) = mpsc::channel(self.task_command_buffer_size);

        self.tasks.insert(
            task_id,
            TaskInfo::Established {
                sender: command_sender,
                connected,
            },
        );

        let mut events = self.events_tx.clone();

        let task = async move {
            loop {
                match futures::future::select(command_receiver.next(), connection.next()).await {
                    Either::Left((Some(command), _)) => match command {
                        task::Command::NotifyHandler(event) => connection.inject_event(event),
                        task::Command::Close => {
                            command_receiver.close();
                            let (handler, closing_muxer) = connection.close();

                            let error = closing_muxer.await.err().map(ConnectionError::IO);
                            events
                                .send(task::Event::Closed {
                                    id: task_id,
                                    error,
                                    handler,
                                })
                                .await;
                            return;
                        }
                    },

                    // The manager has disappeared; abort.
                    Either::Left((None, _)) => return,

                    Either::Right((Some(event), _)) => {
                        match event {
                            Ok(super::Event::Handler(event)) => {
                                events
                                    .send(task::Event::Notify { id: task_id, event })
                                    .await;
                            }
                            Ok(super::Event::AddressChange(new_address)) => {
                                events
                                    .send(task::Event::AddressChange {
                                        id: task_id,
                                        new_address,
                                    })
                                    .await;
                            }
                            Err(error) => {
                                command_receiver.close();
                                let (handler, _closing_muxer) = connection.close();
                                // Terminate the task with the error, dropping the connection.
                                events
                                    .send(task::Event::Closed {
                                        id: task_id,
                                        error: Some(error),
                                        handler,
                                    })
                                    .await;
                                return;
                            }
                        }
                    }
                    Either::Right((None, _)) => {
                        unreachable!("Connection is an infinite stream");
                    }
                }
            }
        }
        .boxed();

        self.spawn(task);
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
        matches!(self.tasks.get(&id.0), Some(TaskInfo::Established { .. }))
    }

    /// Polls the manager for events relating to the managed connections.
    pub fn poll<'a>(&'a mut self, cx: &mut Context<'_>) -> Poll<Event<'a, H, TMuxer, TE>> {
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
                task::Event::IncomingEstablished { id, peer_id, muxer } => {
                    task.remove();
                    Event::IncomingConnectionEstablished {
                        id: ConnectionId(id),
                        peer_id,
                        muxer,
                    }
                }
                task::Event::OutgoingEstablished {
                    id,
                    peer_id,
                    address,
                    muxer,
                    errors,
                } => {
                    task.remove();
                    Event::OutgoingConnectionEstablished {
                        id: ConnectionId(id),
                        peer_id,
                        address,
                        muxer,
                        errors,
                    }
                }
                task::Event::Notify { id: _, event } => Event::ConnectionEvent {
                    entry: EstablishedEntry { task },
                    event,
                },
                task::Event::PendingFailed { id, error } => {
                    let id = ConnectionId(id);
                    let _ = task.remove();
                    Event::PendingConnectionError { id, error }
                }
                task::Event::AddressChange { id: _, new_address } => {
                    let (new, old) =
                        if let TaskInfo::Established { connected, .. } = &mut task.get_mut() {
                            let mut new_endpoint = connected.endpoint.clone();
                            new_endpoint.set_remote_address(new_address);
                            let old_endpoint =
                                mem::replace(&mut connected.endpoint, new_endpoint.clone());
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
                task::Event::Closed { id, error, handler } => {
                    let id = ConnectionId(id);
                    match task.remove() {
                        TaskInfo::Established { sender, connected } => Event::ConnectionClosed {
                            id,
                            connected,
                            error,
                            handler,
                        },
                        TaskInfo::Pending { .. } => {
                            unreachable!("`Event::Closed` is never emitted for pending connection.")
                        }
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
        match &task.get() {
            TaskInfo::Pending { .. } => Entry::Pending(PendingEntry { task }),
            TaskInfo::Established { .. } => Entry::Established(EstablishedEntry { task }),
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
        let cmd = task::Command::NotifyHandler(event);
        match self.task.get_mut() {
            TaskInfo::Established { sender, .. } => {
                sender.try_send(cmd).map_err(|e| match e.into_inner() {
                    task::Command::NotifyHandler(event) => event,
                    _ => unreachable!("Expect `EstablishedEntry` to point to established task."),
                })
            }
            TaskInfo::Pending { .. } => {
                unreachable!("Expect `EstablishedEntry` to point to established task.")
            }
        }
    }

    /// Checks if `notify_handler` is ready to accept an event.
    ///
    /// Returns `Ok(())` if the handler is ready to receive an event via `notify_handler`.
    ///
    /// Returns `Err(())` if the background task associated with the connection
    /// is terminating and the connection is about to close.
    pub fn poll_ready_notify_handler(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), ()>> {
        match self.task.get_mut() {
            TaskInfo::Established { sender, .. } => sender.poll_ready(cx).map_err(|_| ()),
            TaskInfo::Pending { .. } => {
                unreachable!("Expect `EstablishedEntry` to point to established task.")
            }
        }
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
        match self.task.get_mut() {
            TaskInfo::Established { sender, .. } => {
                match sender.clone().try_send(task::Command::Close) {
                    Ok(()) => {}
                    Err(e) => assert!(e.is_disconnected(), "No capacity for close command."),
                }
            }
            TaskInfo::Pending { .. } => {
                unreachable!("Expect `EstablishedEntry` to point to established task.")
            }
        }
    }

    /// Obtains information about the established connection.
    pub fn connected(&self) -> &Connected {
        match &self.task.get() {
            TaskInfo::Established { connected, .. } => connected,
            TaskInfo::Pending { .. } => unreachable!("By Entry::new()"),
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
