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

use super::concurrent_dial::ConcurrentDial;
use crate::{
    connection::{
        self, ConnectionError, PendingInboundConnectionError, PendingOutboundConnectionError,
    },
    transport::{Transport, TransportError},
    ConnectionHandler, Multiaddr, PeerId,
};
use futures::{
    channel::{mpsc, oneshot},
    future::{poll_fn, Either, Future},
    SinkExt, StreamExt,
};
use libp2p_core::connection::ConnectionId;
use std::pin::Pin;
use void::Void;

/// Commands that can be sent to a task driving an established connection.
#[derive(Debug)]
pub enum Command<T> {
    /// Notify the connection handler of an event.
    NotifyHandler(T),
    /// Gracefully close the connection (active close) before
    /// terminating the task.
    Close,
}

#[derive(Debug)]
pub enum PendingConnectionEvent<TTrans>
where
    TTrans: Transport,
{
    ConnectionEstablished {
        id: ConnectionId,
        output: TTrans::Output,
        /// [`Some`] when the new connection is an outgoing connection.
        /// Addresses are dialed in parallel. Contains the addresses and errors
        /// of dial attempts that failed before the one successful dial.
        outgoing: Option<(Multiaddr, Vec<(Multiaddr, TransportError<TTrans::Error>)>)>,
    },
    /// A pending connection failed.
    PendingFailed {
        id: ConnectionId,
        error: Either<
            PendingOutboundConnectionError<TTrans::Error>,
            PendingInboundConnectionError<TTrans::Error>,
        >,
    },
}

#[derive(Debug)]
pub enum EstablishedConnectionEvent<THandler: ConnectionHandler> {
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
        event: THandler::OutEvent,
    },
    /// A connection closed, possibly due to an error.
    ///
    /// If `error` is `None`, the connection has completed
    /// an active orderly close.
    Closed {
        id: ConnectionId,
        peer_id: PeerId,
        error: Option<ConnectionError<THandler::Error>>,
        handler: THandler,
    },
}

pub async fn new_for_pending_outgoing_connection<TTrans>(
    connection_id: ConnectionId,
    dial: ConcurrentDial<TTrans>,
    abort_receiver: oneshot::Receiver<Void>,
    mut events: mpsc::Sender<PendingConnectionEvent<TTrans>>,
) where
    TTrans: Transport,
{
    match futures::future::select(abort_receiver, Box::pin(dial)).await {
        Either::Left((Err(oneshot::Canceled), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: Either::Left(PendingOutboundConnectionError::Aborted),
                })
                .await;
        }
        Either::Left((Ok(v), _)) => void::unreachable(v),
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

pub async fn new_for_pending_incoming_connection<TFut, TTrans>(
    connection_id: ConnectionId,
    future: TFut,
    abort_receiver: oneshot::Receiver<Void>,
    mut events: mpsc::Sender<PendingConnectionEvent<TTrans>>,
) where
    TTrans: Transport,
    TFut: Future<Output = Result<TTrans::Output, TTrans::Error>> + Send + 'static,
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
        Either::Left((Ok(v), _)) => void::unreachable(v),
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

pub async fn new_for_established_connection<THandler>(
    connection_id: ConnectionId,
    peer_id: PeerId,
    mut connection: crate::connection::Connection<THandler>,
    mut command_receiver: mpsc::Receiver<Command<THandler::InEvent>>,
    mut events: mpsc::Sender<EstablishedConnectionEvent<THandler>>,
) where
    THandler: ConnectionHandler,
{
    loop {
        match futures::future::select(
            command_receiver.next(),
            poll_fn(|cx| Pin::new(&mut connection).poll(cx)),
        )
        .await
        {
            Either::Left((Some(command), _)) => match command {
                Command::NotifyHandler(event) => connection.inject_event(event),
                Command::Close => {
                    command_receiver.close();
                    let (handler, closing_muxer) = connection.close();

                    let error = closing_muxer.await.err().map(ConnectionError::IO);
                    let _ = events
                        .send(EstablishedConnectionEvent::Closed {
                            id: connection_id,
                            peer_id,
                            error,
                            handler,
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
                        let (handler, _closing_muxer) = connection.close();
                        // Terminate the task with the error, dropping the connection.
                        let _ = events
                            .send(EstablishedConnectionEvent::Closed {
                                id: connection_id,
                                peer_id,
                                error: Some(error),
                                handler,
                            })
                            .await;
                        return;
                    }
                }
            }
        }
    }
}
