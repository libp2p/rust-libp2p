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

use crate::{
    Multiaddr,
    muxing::StreamMuxer,
    connection::{
        self,
        Close,
        Connected,
        Connection,
        ConnectionError,
        ConnectionHandler,
        IntoConnectionHandler,
        PendingConnectionError,
        Substream,
    },
};
use futures::{prelude::*, channel::mpsc, stream};
use std::{pin::Pin, task::Context, task::Poll};
use super::ConnectResult;

/// Identifier of a [`Task`] in a [`Manager`](super::Manager).
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(pub(super) usize);

/// Commands that can be sent to a [`Task`].
#[derive(Debug)]
pub enum Command<T> {
    /// Notify the connection handler of an event.
    NotifyHandler(T),
    /// Gracefully close the connection (active close) before
    /// terminating the task.
    Close,
}

/// Events that a task can emit to its manager.
#[derive(Debug)]
pub enum Event<T, H, TE, HE, C> {
    /// A connection to a node has succeeded.
    Established { id: TaskId, info: Connected<C> },
    /// A pending connection failed.
    Failed { id: TaskId, error: PendingConnectionError<TE>, handler: H },
    /// A node we are connected to has changed its address.
    AddressChange { id: TaskId, new_address: Multiaddr },
    /// Notify the manager of an event from the connection.
    Notify { id: TaskId, event: T },
    /// A connection closed, possibly due to an error.
    ///
    /// If `error` is `None`, the connection has completed
    /// an active orderly close.
    Closed { id: TaskId, error: Option<ConnectionError<HE>> }
}

impl<T, H, TE, HE, C> Event<T, H, TE, HE, C> {
    pub fn id(&self) -> &TaskId {
        match self {
            Event::Established { id, .. } => id,
            Event::Failed { id, .. } => id,
            Event::AddressChange { id, .. } => id,
            Event::Notify { id, .. } => id,
            Event::Closed { id, .. } => id,
        }
    }
}

/// A `Task` is a [`Future`] that handles a single connection.
pub struct Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoConnectionHandler<C>,
    H::Handler: ConnectionHandler<Substream = Substream<M>>
{
    /// The ID of this task.
    id: TaskId,

    /// Sender to emit events to the manager of this task.
    events: mpsc::Sender<Event<O, H, E, <H::Handler as ConnectionHandler>::Error, C>>,

    /// Receiver for commands sent by the manager of this task.
    commands: stream::Fuse<mpsc::Receiver<Command<I>>>,

    /// Inner state of this `Task`.
    state: State<F, M, H, O, E, C>,
}

impl<F, M, H, I, O, E, C> Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoConnectionHandler<C>,
    H::Handler: ConnectionHandler<Substream = Substream<M>>
{
    /// Create a new task to connect and handle some node.
    pub fn pending(
        id: TaskId,
        events: mpsc::Sender<Event<O, H, E, <H::Handler as ConnectionHandler>::Error, C>>,
        commands: mpsc::Receiver<Command<I>>,
        future: F,
        handler: H
    ) -> Self {
        Task {
            id,
            events,
            commands: commands.fuse(),
            state: State::Pending {
                future: Box::pin(future),
                handler,
            },
        }
    }

    /// Create a task for an existing node we are already connected to.
    pub fn established(
        id: TaskId,
        events: mpsc::Sender<Event<O, H, E, <H::Handler as ConnectionHandler>::Error, C>>,
        commands: mpsc::Receiver<Command<I>>,
        connection: Connection<M, H::Handler>
    ) -> Self {
        Task {
            id,
            events,
            commands: commands.fuse(),
            state: State::Established { connection, event: None },
        }
    }
}

/// The state associated with the `Task` of a connection.
enum State<F, M, H, O, E, C>
where
    M: StreamMuxer,
    H: IntoConnectionHandler<C>,
    H::Handler: ConnectionHandler<Substream = Substream<M>>
{
    /// The connection is being negotiated.
    Pending {
        /// The future that will attempt to reach the node.
        // TODO: don't pin this Future; this requires deeper changes though
        future: Pin<Box<F>>,
        /// The intended handler for the established connection.
        handler: H,
    },

    /// The connection is established.
    Established {
        connection: Connection<M, H::Handler>,
        /// An event to send to the `Manager`. If `None`, the `connection`
        /// is polled for new events in this state, otherwise the event
        /// must be sent to the `Manager` before the connection can be
        /// polled again.
        event: Option<Event<O, H, E, <H::Handler as ConnectionHandler>::Error, C>>
    },

    /// The connection is closing (active close).
    Closing(Close<M>),

    /// The task is terminating with a final event for the `Manager`.
    Terminating(Event<O, H, E, <H::Handler as ConnectionHandler>::Error, C>),

    /// The task has finished.
    Done
}

impl<F, M, H, I, O, E, C> Unpin for Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoConnectionHandler<C>,
    H::Handler: ConnectionHandler<Substream = Substream<M>>
{
}

impl<F, M, H, I, O, E, C> Future for Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    F: Future<Output = ConnectResult<C, M, E>>,
    H: IntoConnectionHandler<C>,
    H::Handler: ConnectionHandler<Substream = Substream<M>, InEvent = I, OutEvent = O>
{
    type Output = ();

    // NOTE: It is imperative to always consume all incoming commands from
    // the manager first, in order to not prevent it from making progress because
    // it is blocked on the channel capacity.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = &mut *self;
        let id = this.id;

        'poll: loop {
            match std::mem::replace(&mut this.state, State::Done) {
                State::Pending { mut future, handler } => {
                    // Check whether the task is still registered with a `Manager`
                    // by polling the commands channel.
                    match this.commands.poll_next_unpin(cx) {
                        Poll::Pending => {},
                        Poll::Ready(None) => {
                            // The manager has dropped the task; abort.
                            return Poll::Ready(())
                        }
                        Poll::Ready(Some(_)) => panic!(
                            "Task received command while the connection is pending."
                        )
                    }
                    // Check if the connection succeeded.
                    match future.poll_unpin(cx) {
                        Poll::Ready(Ok((info, muxer))) => {
                            this.state = State::Established {
                                connection: Connection::new(
                                    muxer,
                                    handler.into_handler(&info),
                                ),
                                event: Some(Event::Established { id, info })
                            }
                        }
                        Poll::Pending => {
                            this.state = State::Pending { future, handler };
                            return Poll::Pending
                        }
                        Poll::Ready(Err(error)) => {
                            // Don't accept any further commands and terminate the
                            // task with a final event.
                            this.commands.get_mut().close();
                            let event = Event::Failed { id, handler, error };
                            this.state = State::Terminating(event)
                        }
                    }
                }

                State::Established { mut connection, event } => {
                    // Check for commands from the `Manager`.
                    loop {
                        match this.commands.poll_next_unpin(cx) {
                            Poll::Pending => break,
                            Poll::Ready(Some(Command::NotifyHandler(event))) =>
                                connection.inject_event(event),
                            Poll::Ready(Some(Command::Close)) => {
                                // Don't accept any further commands.
                                this.commands.get_mut().close();
                                // Discard the event, if any, and start a graceful close.
                                this.state = State::Closing(connection.close());
                                continue 'poll
                            }
                            Poll::Ready(None) => {
                                // The manager has dropped the task or disappeared; abort.
                                return Poll::Ready(())
                            }
                        }
                    }

                    if let Some(event) = event {
                        // Send the event to the manager.
                        match this.events.poll_ready(cx) {
                            Poll::Pending => {
                                this.state = State::Established { connection, event: Some(event) };
                                return Poll::Pending
                            }
                            Poll::Ready(result) => {
                                if result.is_ok() {
                                    if let Ok(()) = this.events.start_send(event) {
                                        this.state = State::Established { connection, event: None };
                                        continue 'poll
                                    }
                                }
                                // The manager is no longer reachable; abort.
                                return Poll::Ready(())
                            }
                        }
                    } else {
                        // Poll the connection for new events.
                        match Connection::poll(Pin::new(&mut connection), cx) {
                            Poll::Pending => {
                                this.state = State::Established { connection, event: None };
                                return Poll::Pending
                            }
                            Poll::Ready(Ok(connection::Event::Handler(event))) => {
                                this.state = State::Established {
                                    connection,
                                    event: Some(Event::Notify { id, event })
                                };
                            }
                            Poll::Ready(Ok(connection::Event::AddressChange(new_address))) => {
                                this.state = State::Established {
                                    connection,
                                    event: Some(Event::AddressChange { id, new_address })
                                };
                            }
                            Poll::Ready(Err(error)) => {
                                // Don't accept any further commands.
                                this.commands.get_mut().close();
                                // Terminate the task with the error, dropping the connection.
                                let event = Event::Closed { id, error: Some(error) };
                                this.state = State::Terminating(event);
                            }
                        }
                    }
                }

                State::Closing(mut closing) => {
                    // Try to gracefully close the connection.
                    match closing.poll_unpin(cx) {
                        Poll::Ready(Ok(())) => {
                            let event = Event::Closed { id: this.id, error: None };
                            this.state = State::Terminating(event);
                        }
                        Poll::Ready(Err(e)) => {
                            let event = Event::Closed {
                                id: this.id,
                                error: Some(ConnectionError::IO(e))
                            };
                            this.state = State::Terminating(event);
                        }
                        Poll::Pending => {
                            this.state = State::Closing(closing);
                            return Poll::Pending
                        }
                    }
                }

                State::Terminating(event) => {
                    // Try to deliver the final event.
                    match this.events.poll_ready(cx) {
                        Poll::Pending => {
                            self.state = State::Terminating(event);
                            return Poll::Pending
                        }
                        Poll::Ready(result) => {
                            if result.is_ok() {
                                let _ = this.events.start_send(event);
                            }
                            return Poll::Ready(())
                        }
                    }
                }

                State::Done => panic!("`Task::poll()` called after completion.")
            }
        }
    }
}
