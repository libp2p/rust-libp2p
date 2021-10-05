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
        Connected, ConnectionError, ConnectionHandler, ConnectionLimit, Endpoint, IncomingInfo,
        IntoConnectionHandler, OutgoingInfo, PendingConnectionError, PendingPoint, Substream,
    },
    muxing::StreamMuxer,
    network::DialError,
    transport::TransportError,
    ConnectedPoint, Executor, Multiaddr, PeerId,
};
use fnv::FnvHashMap;
use futures::prelude::*;
use futures::{
    channel::{mpsc, oneshot},
    future::Either,
    future::{poll_fn, BoxFuture},
    prelude::*,
    ready,
    stream::FuturesUnordered,
};
use smallvec::SmallVec;
use std::{
    collections::HashMap, convert::TryFrom as _, error, fmt, num::NonZeroU32, pin::Pin,
    task::Context, task::Poll,
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

/// Events that a task can emit to its manager.
#[derive(Debug)]
pub enum Event<THandler: IntoConnectionHandler, TMuxer, TError> {
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
    /// A node we are connected to has changed its address.
    AddressChange {
        id: ConnectionId,
        new_address: Multiaddr,
    },
    /// Notify the manager of an event from the connection.
    Notify {
        id: ConnectionId,
        event: THandlerOutEvent<THandler>,
    },
    /// A connection closed, possibly due to an error.
    ///
    /// If `error` is `None`, the connection has completed
    /// an active orderly close.
    Closed {
        id: ConnectionId,
        error: Option<ConnectionError<THandlerError<THandler>>>,
        handler: THandler::Handler,
    },
}

// TODO: Still needed?
impl<H: IntoConnectionHandler, TMuxer, TError> Event<H, TMuxer, TError> {
    pub fn id(&self) -> &ConnectionId {
        match self {
            Event::ConnectionEstablished { id, .. } => id,
            Event::PendingFailed { id, .. } => id,
            Event::AddressChange { id, .. } => id,
            Event::Notify { id, .. } => id,
            Event::Closed { id, .. } => id,
        }
    }
}
pub async fn new_for_pending_outgoing_connection<THandler, TMuxer, TTransErr>(
    connection_id: ConnectionId,
    dial: crate::network::concurrent_dial::ConcurrentDial<TMuxer, TTransErr>,
    drop_receiver: oneshot::Receiver<Void>,
    mut events: mpsc::Sender<Event<THandler, TMuxer, TTransErr>>,
) where
    THandler: IntoConnectionHandler + Send + 'static,
{
    match futures::future::select(drop_receiver, Box::pin(dial)).await {
        Either::Left((Err(oneshot::Canceled), _)) => {
            events
                .send(Event::PendingFailed {
                    id: connection_id,
                    error: PendingConnectionError::Aborted,
                })
                .await;
        }
        Either::Left((Ok(v), _)) => void::unreachable(v),
        Either::Right((Ok((peer_id, address, muxer, errors)), _)) => {
            events
                .send(Event::ConnectionEstablished {
                    id: connection_id,
                    peer_id,
                    muxer,
                    outgoing: Some((address, errors)),
                })
                .await;
        }
        Either::Right((Err(e), _)) => {
            events
                .send(Event::PendingFailed {
                    id: connection_id,
                    error: PendingConnectionError::TransportDial(e),
                })
                .await;
        }
    }
}

pub async fn new_for_pending_incoming_connection<TFut, THandler, TMuxer, TTransErr>(
    connection_id: ConnectionId,
    future: TFut,
    drop_receiver: oneshot::Receiver<Void>,
    mut events: mpsc::Sender<Event<THandler, TMuxer, TTransErr>>,
) where
    TFut: Future<Output = Result<(PeerId, TMuxer), PendingConnectionError<TTransErr>>>
        + Send
        + 'static,
    THandler: IntoConnectionHandler + Send + 'static,
{
    match futures::future::select(drop_receiver, Box::pin(future)).await {
        Either::Left((Err(oneshot::Canceled), _)) => {
            events
                .send(Event::PendingFailed {
                    id: connection_id,
                    error: PendingConnectionError::Aborted,
                })
                .await;
        }
        Either::Left((Ok(v), _)) => void::unreachable(v),
        Either::Right((Ok((peer_id, muxer)), _)) => {
            events
                .send(Event::ConnectionEstablished {
                    id: connection_id,
                    peer_id,
                    muxer,
                    outgoing: None,
                })
                .await;
        }
        Either::Right((Err(e), _)) => {
            events
                .send(Event::PendingFailed {
                    id: connection_id,
                    error: e,
                })
                .await;
        }
    }
}

pub async fn new_for_established_connection<TMuxer, THandler, TTransErr>(
    connection_id: ConnectionId,
    mut connection: crate::connection::Connection<TMuxer, THandler::Handler>,
    mut command_receiver: mpsc::Receiver<Command<THandlerInEvent<THandler>>>,
    mut events: mpsc::Sender<Event<THandler, TMuxer, TTransErr>>,
) where
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send + 'static,
    THandler: IntoConnectionHandler,
    THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>> + Send + 'static,
    <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static,
{
    loop {
        match futures::future::select(command_receiver.next(), connection.next()).await {
            Either::Left((Some(command), _)) => match command {
                Command::NotifyHandler(event) => connection.inject_event(event),
                Command::Close => {
                    command_receiver.close();
                    let (handler, closing_muxer) = connection.close();

                    let error = closing_muxer.await.err().map(ConnectionError::IO);
                    events
                        .send(Event::Closed {
                            id: connection_id,
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
                        events
                            .send(Event::Notify {
                                id: connection_id,
                                event,
                            })
                            .await;
                    }
                    Ok(connection::Event::AddressChange(new_address)) => {
                        events
                            .send(Event::AddressChange {
                                id: connection_id,
                                new_address,
                            })
                            .await;
                    }
                    Err(error) => {
                        command_receiver.close();
                        let (handler, _closing_muxer) = connection.close();
                        // Terminate the task with the error, dropping the connection.
                        events
                            .send(Event::Closed {
                                id: connection_id,
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
