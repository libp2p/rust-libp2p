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
    muxing::StreamMuxer,
    connection::{
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
}

/// Events that a task can emit to its manager.
#[derive(Debug)]
pub enum Event<T, H, TE, HE, C> {
    /// A connection to a node has succeeded.
    Established { id: TaskId, info: Connected<C> },
    /// An established connection produced an error.
    Error { id: TaskId, error: ConnectionError<HE> },
    /// A pending connection failed.
    Failed { id: TaskId, error: PendingConnectionError<TE>, handler: H },
    /// Notify the manager of an event from the connection.
    Notify { id: TaskId, event: T }
}

impl<T, H, TE, HE, C> Event<T, H, TE, HE, C> {
    pub fn id(&self) -> &TaskId {
        match self {
            Event::Established { id, .. } => id,
            Event::Error { id, .. } => id,
            Event::Notify { id, .. } => id,
            Event::Failed { id, .. } => id,
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
            state: State::EstablishedPending(connection),
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
    /// The task is waiting for the connection to be established.
    Pending {
        /// The future that will attempt to reach the node.
        // TODO: don't pin this Future; this requires deeper changes though
        future: Pin<Box<F>>,
        /// The intended handler for the established connection.
        handler: H,
    },

    /// The connection is established and a new event is ready to be emitted.
    EstablishedReady {
        /// The node, if available.
        connection: Option<Connection<M, H::Handler>>,
        /// The actual event message to send.
        event: Event<O, H, E, <H::Handler as ConnectionHandler>::Error, C>
    },

    /// The connection is established and pending a new event to occur.
    EstablishedPending(Connection<M, H::Handler>),

    /// The task is closing the connection.
    Closing(Close<M>),

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
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let this = &mut *self;
        let id = this.id;

        'poll: loop {
            match std::mem::replace(&mut this.state, State::Done) {
                State::Pending { mut future, handler } => {
                    // Check if the manager aborted this task by dropping the `commands`
                    // channel sender side.
                    match Stream::poll_next(Pin::new(&mut this.commands), cx) {
                        Poll::Pending => {},
                        Poll::Ready(None) => return Poll::Ready(()),
                        Poll::Ready(Some(Command::NotifyHandler(_))) => unreachable!(
                            "Manager does not allow sending commands to pending tasks.",
                        )
                    }
                    // Check if the connection succeeded.
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Ready(Ok((info, muxer))) => {
                            this.state = State::EstablishedReady {
                                connection: Some(Connection::new(
                                    muxer,
                                    handler.into_handler(&info),
                                )),
                                event: Event::Established { id, info }
                            }
                        }
                        Poll::Pending => {
                            this.state = State::Pending { future, handler };
                            return Poll::Pending
                        }
                        Poll::Ready(Err(error)) => {
                            let event = Event::Failed { id, handler, error };
                            this.state = State::EstablishedReady { connection: None, event }
                        }
                    }
                }

                State::EstablishedPending(mut connection) => {
                    // Start by handling commands received from the manager, if any.
                    loop {
                        match Stream::poll_next(Pin::new(&mut this.commands), cx) {
                            Poll::Pending => break,
                            Poll::Ready(Some(Command::NotifyHandler(event))) =>
                                connection.inject_event(event),
                            Poll::Ready(None) => {
                                // The manager has dropped the task, thus initiate a
                                // graceful shutdown of the connection.
                                this.state = State::Closing(connection.close());
                                continue 'poll
                            }
                        }
                    }
                    // Poll the connection for new events.
                    loop {
                        match Connection::poll(Pin::new(&mut connection), cx) {
                            Poll::Pending => {
                                this.state = State::EstablishedPending(connection);
                                return Poll::Pending
                            }
                            Poll::Ready(Ok(event)) => {
                                this.state = State::EstablishedReady {
                                    connection: Some(connection),
                                    event: Event::Notify { id, event }
                                };
                                continue 'poll
                            }
                            Poll::Ready(Err(error)) => {
                                // Notify the manager of the error via an event,
                                // dropping the connection.
                                let event = Event::Error { id, error };
                                this.state = State::EstablishedReady { connection: None, event };
                                continue 'poll
                            }
                        }
                    }
                }

                // Deliver an event to the manager.
                State::EstablishedReady { mut connection, event } => {
                    // Process commands received from the manager, if any.
                    loop {
                        match Stream::poll_next(Pin::new(&mut this.commands), cx) {
                            Poll::Pending => break,
                            Poll::Ready(Some(Command::NotifyHandler(event))) =>
                                if let Some(ref mut c) = connection {
                                    c.inject_event(event)
                                }
                            Poll::Ready(None) =>
                                // The manager has dropped the task, thus initiate a
                                // graceful shutdown of the connection, if given.
                                if let Some(c) = connection {
                                    this.state = State::Closing(c.close());
                                    continue 'poll
                                } else {
                                    return Poll::Ready(())
                                }
                        }
                    }
                    // Send the event to the manager.
                    match this.events.poll_ready(cx) {
                        Poll::Pending => {
                            self.state = State::EstablishedReady { connection, event };
                            return Poll::Pending
                        }
                        Poll::Ready(Ok(())) => {
                            // We assume that if `poll_ready` has succeeded, then sending the event
                            // will succeed as well. If it turns out that it didn't, we will detect
                            // the closing at the next loop iteration.
                            let _ = this.events.start_send(event);
                            if let Some(c) = connection {
                                this.state = State::EstablishedPending(c)
                            } else {
                                // The connection has been dropped, thus this was the last event
                                // to send to the manager and the task is done.
                                return Poll::Ready(())
                            }
                        },
                        Poll::Ready(Err(_)) => {
                            // The manager is no longer reachable, maybe due to
                            // application shutdown. Try a graceful shutdown of the
                            // connection, if available, and end the task.
                            if let Some(c) = connection {
                                this.state = State::Closing(c.close());
                                continue 'poll
                            }
                            return Poll::Ready(())
                        }
                    }
                }

                State::Closing(mut closing) =>
                    match Future::poll(Pin::new(&mut closing), cx) {
                        Poll::Ready(_) => return Poll::Ready(()), // end task
                        Poll::Pending => {
                            this.state = State::Closing(closing);
                            return Poll::Pending
                        }
                    }

                State::Done => panic!("`Task::poll()` called after completion.")
            }
        }
    }
}
