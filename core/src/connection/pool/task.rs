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

use super::ConnectionId;
use crate::{
    connection::{
        self,
        handler::{THandlerError, THandlerInEvent, THandlerOutEvent},
        ConnectionError, ConnectionHandler, IntoConnectionHandler, PendingConnectionError,
        Substream,
    },
    muxing::StreamMuxer,
    transport::TransportError,
    Multiaddr, PeerId,
};
use futures::{
    channel::{mpsc, oneshot},
    future::{Either, Future},
    SinkExt, StreamExt,
};
use void::Void;

/// Commands that can be sent to a task.
#[derive(Debug)]
pub enum Command<T> {
    /// Notify the connection handler of an event.
    NotifyHandler(T),
    /// Gracefully close the connection (active close) before
    /// terminating the task.
    Close,
}

#[derive(Debug)]
pub enum PendingConnectionEvent<TMuxer, TError> {
    // TODO: Remove most of these
    ConnectionEstablished {
        id: ConnectionId,
        peer_id: PeerId,
        muxer: TMuxer,
        // TODO: Document errors in a success message.
        outgoing: Option<(Multiaddr, Vec<(Multiaddr, TransportError<TError>)>)>,
    },
    /// A pending connection failed.
    PendingFailed {
        id: ConnectionId,
        error: PendingConnectionError<TError>,
    },
}

#[derive(Debug)]
pub enum EstablishedConnectionEvent<THandler: IntoConnectionHandler> {
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
        event: THandlerOutEvent<THandler>,
    },
    /// A connection closed, possibly due to an error.
    ///
    /// If `error` is `None`, the connection has completed
    /// an active orderly close.
    Closed {
        id: ConnectionId,
        peer_id: PeerId,
        error: Option<ConnectionError<THandlerError<THandler>>>,
        handler: THandler::Handler,
    },
}

pub async fn new_for_pending_outgoing_connection<TMuxer, TTransErr>(
    connection_id: ConnectionId,
    dial: crate::network::concurrent_dial::ConcurrentDial<TMuxer, TTransErr>,
    drop_receiver: oneshot::Receiver<Void>,
    mut events: mpsc::Sender<PendingConnectionEvent<TMuxer, TTransErr>>,
) {
    match futures::future::select(drop_receiver, Box::pin(dial)).await {
        Either::Left((Err(oneshot::Canceled), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: PendingConnectionError::Aborted,
                })
                .await;
        }
        Either::Left((Ok(v), _)) => void::unreachable(v),
        Either::Right((Ok((peer_id, address, muxer, errors)), _)) => {
            let _ = events
                .send(PendingConnectionEvent::ConnectionEstablished {
                    id: connection_id,
                    peer_id,
                    muxer,
                    outgoing: Some((address, errors)),
                })
                .await;
        }
        Either::Right((Err(e), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: PendingConnectionError::TransportDial(e),
                })
                .await;
        }
    }
}

pub async fn new_for_pending_incoming_connection<TFut, TMuxer, TTransErr>(
    connection_id: ConnectionId,
    future: TFut,
    drop_receiver: oneshot::Receiver<Void>,
    mut events: mpsc::Sender<PendingConnectionEvent<TMuxer, TTransErr>>,
) where
    TFut: Future<Output = Result<(PeerId, TMuxer), TTransErr>> + Send + 'static,
{
    match futures::future::select(drop_receiver, Box::pin(future)).await {
        Either::Left((Err(oneshot::Canceled), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: PendingConnectionError::Aborted,
                })
                .await;
        }
        Either::Left((Ok(v), _)) => void::unreachable(v),
        Either::Right((Ok((peer_id, muxer)), _)) => {
            let _ = events
                .send(PendingConnectionEvent::ConnectionEstablished {
                    id: connection_id,
                    peer_id,
                    muxer,
                    outgoing: None,
                })
                .await;
        }
        Either::Right((Err(e), _)) => {
            let _ = events
                .send(PendingConnectionEvent::PendingFailed {
                    id: connection_id,
                    error: PendingConnectionError::TransportListen(TransportError::Other(e)),
                })
                .await;
        }
    }
}

pub async fn new_for_established_connection<TMuxer, THandler>(
    connection_id: ConnectionId,
    peer_id: PeerId,
    mut connection: crate::connection::Connection<TMuxer, THandler::Handler>,
    mut command_receiver: mpsc::Receiver<Command<THandlerInEvent<THandler>>>,
    mut events: mpsc::Sender<EstablishedConnectionEvent<THandler>>,
) where
    TMuxer: StreamMuxer,
    THandler: IntoConnectionHandler,
    THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>>,
{
    loop {
        match futures::future::select(command_receiver.next(), connection.next()).await {
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

            Either::Right((Some(event), _)) => {
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
            Either::Right((None, _)) => {
                unreachable!("Connection is an infinite stream");
            }
        }
    }
}
