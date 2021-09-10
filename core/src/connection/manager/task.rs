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

use super::ConnectResult;
use crate::{
    connection::{
        self,
        handler::{THandlerError, THandlerInEvent, THandlerOutEvent},
        Close, Connected, Connection, ConnectionError, ConnectionHandler, ConnectionLimit,
        IntoConnectionHandler, PendingConnectionError, Substream,
    },
    muxing::StreamMuxer,
    Multiaddr,
};
use futures::{channel::mpsc, prelude::*, stream};
use std::{pin::Pin, task::Context, task::Poll};

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
    Close(Option<ConnectionLimit>),
}

/// Events that a task can emit to its manager.
#[derive(Debug)]
pub enum Event<H: IntoConnectionHandler, TE> {
    /// A connection to a node has succeeded.
    Established { id: TaskId, info: Connected },
    /// A pending connection failed.
    Failed {
        id: TaskId,
        error: PendingConnectionError<TE>,
        handler: H,
    },
    /// A node we are connected to has changed its address.
    AddressChange { id: TaskId, new_address: Multiaddr },
    /// Notify the manager of an event from the connection.
    Notify {
        id: TaskId,
        event: THandlerOutEvent<H>,
    },
    /// A connection closed, possibly due to an error.
    ///
    /// If `error` is `None`, the connection has completed
    /// an active orderly close.
    Closed {
        id: TaskId,
        error: Option<ConnectionError<THandlerError<H>>>,
        handler: H::Handler,
    },
}

impl<H: IntoConnectionHandler, TE> Event<H, TE> {
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
pub struct Task<F, M, H, E>
where
    M: StreamMuxer,
    H: IntoConnectionHandler,
    H::Handler: ConnectionHandler<Substream = Substream<M>>,
{
    /// The ID of this task.
    id: TaskId,

    /// Sender to emit events to the manager of this task.
    events: mpsc::Sender<Event<H, E>>,

    /// Receiver for commands sent by the manager of this task.
    commands: stream::Fuse<mpsc::Receiver<Command<THandlerInEvent<H>>>>,

    /// Inner state of this `Task`.
    state: State<F, M, H, E>,
}

impl<F, M, H, E> Task<F, M, H, E>
where
    M: StreamMuxer,
    H: IntoConnectionHandler,
    H::Handler: ConnectionHandler<Substream = Substream<M>>,
{
    /// Create a new task to connect and handle some node.
    pub fn pending(
        id: TaskId,
        events: mpsc::Sender<Event<H, E>>,
        commands: mpsc::Receiver<Command<THandlerInEvent<H>>>,
        future: F,
        handler: H,
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
}

/// The state associated with the `Task` of a connection.
enum State<F, M, H, E>
where
    M: StreamMuxer,
    H: IntoConnectionHandler,
    H::Handler: ConnectionHandler<Substream = Substream<M>>,
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
        event: Option<Event<H, E>>,
    },

    /// The connection is closing (active close).
    Closing {
        closing_muxer: Close<M>,
        handler: H::Handler,
        error: Option<ConnectionLimit>,
    },

    /// The task is terminating with a final event for the `Manager`.
    Terminating(Event<H, E>),

    /// The task has finished.
    Done,
}

impl<F, M, H, E> Unpin for Task<F, M, H, E>
where
    M: StreamMuxer,
    H: IntoConnectionHandler,
    H::Handler: ConnectionHandler<Substream = Substream<M>>,
{
}

impl<F, M, H, E> Future for Task<F, M, H, E>
where
    M: StreamMuxer,
    F: Future<Output = ConnectResult<M, E>>,
    H: IntoConnectionHandler,
    H::Handler: ConnectionHandler<Substream = Substream<M>> + Send + 'static,
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
                State::Pending {
                    mut future,
                    handler,
                } => {
                    // Check whether the task is still registered with a `Manager`
                    // by polling the commands channel.
                    match this.commands.poll_next_unpin(cx) {
                        Poll::Pending => {}
                        Poll::Ready(None) => {
                            // The manager has dropped the task; abort.
                            // Don't accept any further commands and terminate the
                            // task with a final event.
                            this.commands.get_mut().close();
                            let event = Event::Failed {
                                id,
                                handler,
                                error: PendingConnectionError::Aborted,
                            };
                            this.state = State::Terminating(event);
                            continue 'poll;
                        }
                        Poll::Ready(Some(_)) => {
                            panic!("Task received command while the connection is pending.")
                        }
                    }
                    // Check if the connection succeeded.
                    match future.poll_unpin(cx) {
                        Poll::Ready(Ok((info, muxer))) => {
                            this.state = State::Established {
                                connection: Connection::new(muxer, handler.into_handler(&info)),
                                event: Some(Event::Established { id, info }),
                            }
                        }
                        Poll::Pending => {
                            this.state = State::Pending { future, handler };
                            return Poll::Pending;
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

                State::Established {
                    mut connection,
                    event,
                } => {
                    // Check for commands from the `Manager`.
                    loop {
                        match this.commands.poll_next_unpin(cx) {
                            Poll::Pending => break,
                            Poll::Ready(Some(Command::NotifyHandler(event))) => {
                                connection.inject_event(event)
                            }
                            Poll::Ready(Some(Command::Close(error))) => {
                                // Don't accept any further commands.
                                this.commands.get_mut().close();
                                // Discard the event, if any, and start a graceful close.
                                let (handler, closing_muxer) = connection.close();
                                this.state = State::Closing {
                                    handler,
                                    closing_muxer,
                                    error,
                                };
                                continue 'poll;
                            }
                            Poll::Ready(None) => {
                                // The manager has disappeared; abort.
                                return Poll::Ready(());
                            }
                        }
                    }

                    if let Some(event) = event {
                        // Send the event to the manager.
                        match this.events.poll_ready(cx) {
                            Poll::Pending => {
                                this.state = State::Established {
                                    connection,
                                    event: Some(event),
                                };
                                return Poll::Pending;
                            }
                            Poll::Ready(result) => {
                                if result.is_ok() {
                                    if let Ok(()) = this.events.start_send(event) {
                                        this.state = State::Established {
                                            connection,
                                            event: None,
                                        };
                                        continue 'poll;
                                    }
                                }
                                // The manager is no longer reachable; abort.
                                return Poll::Ready(());
                            }
                        }
                    } else {
                        // Poll the connection for new events.
                        match Connection::poll(Pin::new(&mut connection), cx) {
                            Poll::Pending => {
                                this.state = State::Established {
                                    connection,
                                    event: None,
                                };
                                return Poll::Pending;
                            }
                            Poll::Ready(Ok(connection::Event::Handler(event))) => {
                                this.state = State::Established {
                                    connection,
                                    event: Some(Event::Notify { id, event }),
                                };
                            }
                            Poll::Ready(Ok(connection::Event::AddressChange(new_address))) => {
                                this.state = State::Established {
                                    connection,
                                    event: Some(Event::AddressChange { id, new_address }),
                                };
                            }
                            Poll::Ready(Err(error)) => {
                                // Don't accept any further commands.
                                this.commands.get_mut().close();
                                let (handler, _closing_muxer) = connection.close();
                                // Terminate the task with the error, dropping the connection.
                                let event = Event::Closed {
                                    id,
                                    error: Some(error),
                                    handler,
                                };
                                this.state = State::Terminating(event);
                            }
                        }
                    }
                }

                State::Closing {
                    handler,
                    error,
                    mut closing_muxer,
                } => {
                    // Try to gracefully close the connection.
                    match closing_muxer.poll_unpin(cx) {
                        Poll::Ready(Ok(())) => {
                            let event = Event::Closed {
                                id: this.id,
                                error: error.map(|limit| ConnectionError::ConnectionLimit(limit)),
                                handler,
                            };
                            this.state = State::Terminating(event);
                        }
                        Poll::Ready(Err(e)) => {
                            let event = Event::Closed {
                                id: this.id,
                                error: Some(ConnectionError::IO(e)),
                                handler,
                            };
                            this.state = State::Terminating(event);
                        }
                        Poll::Pending => {
                            this.state = State::Closing {
                                handler,
                                error,
                                closing_muxer,
                            };
                            return Poll::Pending;
                        }
                    }
                }

                State::Terminating(event) => {
                    // Try to deliver the final event.
                    match this.events.poll_ready(cx) {
                        Poll::Pending => {
                            self.state = State::Terminating(event);
                            return Poll::Pending;
                        }
                        Poll::Ready(result) => {
                            if result.is_ok() {
                                let _ = this.events.start_send(event);
                            }
                            return Poll::Ready(());
                        }
                    }
                }

                State::Done => panic!("`Task::poll()` called after completion."),
            }
        }
    }
}
