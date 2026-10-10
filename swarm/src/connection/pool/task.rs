// Copyright 2021 Protocol Labs.
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

//! Async functions driving pending and established connections in the form of a task.

use std::{
    convert::Infallible,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    task::Poll,
};

use futures::{
    SinkExt, StreamExt,
    channel::{mpsc, oneshot},
    future::{Either, Future, FutureExt, poll_fn},
};
use libp2p_core::muxing::StreamMuxerBox;

use super::concurrent_dial::DialResult;
use crate::{
    ConnectionHandler, Multiaddr, PeerId,
    connection::{
        self, ConnectionError, ConnectionId, PendingInboundConnectionError,
        PendingOutboundConnectionError,
    },
    transport::TransportError,
};

/// Commands that can be sent to a task driving an established connection.
#[derive(Debug)]
pub(crate) enum Command<T> {
    /// Notify the connection handler of an event.
    NotifyHandler(T),
    /// Gracefully close the connection (active close) before
    /// terminating the task.
    Close,
}

pub(crate) enum PendingConnectionEvent {
    ConnectionEstablished {
        id: ConnectionId,
        output: (PeerId, StreamMuxerBox),
        /// [`Some`] when the new connection is an outgoing connection.
        /// Addresses are dialed in parallel. Contains the addresses and errors
        /// of dial attempts that failed before the one successful dial.
        outgoing: Option<(Multiaddr, Vec<(Multiaddr, TransportError<std::io::Error>)>)>,
    },
    /// A pending connection failed.
    PendingFailed {
        id: ConnectionId,
        error: Either<PendingOutboundConnectionError, PendingInboundConnectionError>,
    },
}

#[derive(Debug)]
pub(crate) enum EstablishedConnectionEvent<ToBehaviour> {
    /// A node we are connected to has changed its address.
    AddressChange {
        id: ConnectionId,
        peer_id: PeerId,
        new_address: Multiaddr,
    },
    /// Notify the manager of an event from the connection.
    Notify {
        id: ConnectionId,
        peer_id: PeerId,
        event: ToBehaviour,
    },
    /// A connection closed, possibly due to an error.
    ///
    /// If `error` is `None`, the connection has completed
    /// an active orderly close.
    Closed {
        id: ConnectionId,
        peer_id: PeerId,
        error: Option<ConnectionError>,
    },
}

pub(crate) async fn new_for_pending_outgoing_connection<D: Future<Output = DialResult>>(
    connection_id: ConnectionId,
    dial: D,
    abort_receiver: oneshot::Receiver<Infallible>,
    mut events: mpsc::Sender<PendingConnectionEvent>,
) {
    match futures::future::select(abort_receiver, Box::pin(dial)).await {
        Either::Left((Err(oneshot::Canceled), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: Either::Left(PendingOutboundConnectionError::Aborted),
                })
                .await;
        }
        Either::Left((Ok(v), _)) => libp2p_core::util::unreachable(v),
        Either::Right((Ok((address, output, errors)), _)) => {
            let _ = events
                .send(PendingConnectionEvent::ConnectionEstablished {
                    id: connection_id,
                    output,
                    outgoing: Some((address, errors)),
                })
                .await;
        }
        Either::Right((Err(e), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: Either::Left(PendingOutboundConnectionError::Transport(e)),
                })
                .await;
        }
    }
}

pub(crate) async fn new_for_pending_incoming_connection<TFut>(
    connection_id: ConnectionId,
    future: TFut,
    abort_receiver: oneshot::Receiver<Infallible>,
    mut events: mpsc::Sender<PendingConnectionEvent>,
) where
    TFut: Future<Output = Result<(PeerId, StreamMuxerBox), std::io::Error>> + Send + 'static,
{
    match futures::future::select(abort_receiver, Box::pin(future)).await {
        Either::Left((Err(oneshot::Canceled), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: Either::Right(PendingInboundConnectionError::Aborted),
                })
                .await;
        }
        Either::Left((Ok(v), _)) => libp2p_core::util::unreachable(v),
        Either::Right((Ok(output), _)) => {
            let _ = events
                .send(PendingConnectionEvent::ConnectionEstablished {
                    id: connection_id,
                    output,
                    outgoing: None,
                })
                .await;
        }
        Either::Right((Err(e), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: Either::Right(PendingInboundConnectionError::Transport(
                        TransportError::Other(e),
                    )),
                })
                .await;
        }
    }
}

pub(crate) async fn new_for_established_connection<THandler>(
    connection_id: ConnectionId,
    peer_id: PeerId,
    mut connection: crate::connection::Connection<THandler>,
    mut command_receiver: mpsc::Receiver<Command<THandler::FromBehaviour>>,
    mut events: mpsc::Sender<EstablishedConnectionEvent<THandler::ToBehaviour>>,
) where
    THandler: ConnectionHandler,
{
    loop {
        let connection_poll = poll_fn(|cx| {
            // A panic in the connection state machine must not unwind into
            // the task. Report the connection as closed instead.
            match catch_unwind(AssertUnwindSafe(|| Pin::new(&mut connection).poll(cx))) {
                Ok(poll) => poll,
                Err(panic) => {
                    let message = panic
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic.downcast_ref::<&str>().copied())
                        .unwrap_or("unknown panic");
                    tracing::error!(
                        ?connection_id,
                        %peer_id,
                        panic_message = message,
                        "Panic in connection state machine; closing the connection"
                    );
                    Poll::Ready(Err(ConnectionError::Panicked(message.to_string())))
                }
            }
        });

        match futures::future::select(command_receiver.next(), connection_poll).await {
            Either::Left((Some(command), _)) => match command {
                Command::NotifyHandler(event) => connection.on_behaviour_event(event),
                Command::Close => {
                    command_receiver.close();
                    let (remaining_events, closing_muxer) = connection.close();

                    let closing = async {
                        let _ = events
                            .send_all(&mut remaining_events.map(|event| {
                                Ok(EstablishedConnectionEvent::Notify {
                                    id: connection_id,
                                    event,
                                    peer_id,
                                })
                            }))
                            .await;

                        closing_muxer.await
                    };

                    // A panic in the handler or muxer close must not unwind
                    // into the task. Report the connection as closed instead.
                    let error = match AssertUnwindSafe(closing).catch_unwind().await {
                        Ok(result) => result.err().map(ConnectionError::IO),
                        Err(panic) => {
                            let message = panic
                                .downcast_ref::<String>()
                                .map(String::as_str)
                                .or_else(|| panic.downcast_ref::<&str>().copied())
                                .unwrap_or("unknown panic");
                            tracing::error!(
                                ?connection_id,
                                %peer_id,
                                panic_message = message,
                                "Panic while closing the connection"
                            );
                            Some(ConnectionError::Panicked(message.to_string()))
                        }
                    };

                    let _ = events
                        .send(EstablishedConnectionEvent::Closed {
                            id: connection_id,
                            peer_id,
                            error,
                        })
                        .await;
                    return;
                }
            },

            // The manager has disappeared; abort.
            Either::Left((None, _)) => return,

            Either::Right((event, _)) => {
                match event {
                    Ok(connection::Event::Handler(event)) => {
                        let _ = events
                            .send(EstablishedConnectionEvent::Notify {
                                id: connection_id,
                                peer_id,
                                event,
                            })
                            .await;
                    }
                    Ok(connection::Event::AddressChange(new_address)) => {
                        let _ = events
                            .send(EstablishedConnectionEvent::AddressChange {
                                id: connection_id,
                                peer_id,
                                new_address,
                            })
                            .await;
                    }
                    Err(error) => {
                        command_receiver.close();
                        let (remaining_events, _closing_muxer) = connection.close();

                        // A panic in the handler close must not prevent the
                        // closed event from being reported.
                        let drain = async {
                            let _ = events
                                .send_all(&mut remaining_events.map(|event| {
                                    Ok(EstablishedConnectionEvent::Notify {
                                        id: connection_id,
                                        event,
                                        peer_id,
                                    })
                                }))
                                .await;
                        };
                        if let Err(panic) = AssertUnwindSafe(drain).catch_unwind().await {
                            let message = panic
                                .downcast_ref::<String>()
                                .map(String::as_str)
                                .or_else(|| panic.downcast_ref::<&str>().copied())
                                .unwrap_or("unknown panic");
                            tracing::error!(
                                ?connection_id,
                                %peer_id,
                                panic_message = message,
                                "Panic while draining handler events after a connection error"
                            );
                        }

                        // Terminate the task with the error, dropping the connection.
                        let _ = events
                            .send(EstablishedConnectionEvent::Closed {
                                id: connection_id,
                                peer_id,
                                error: Some(error),
                            })
                            .await;
                        return;
                    }
                }
            }
        }
    }
}
