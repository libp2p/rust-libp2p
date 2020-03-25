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

//! Network events and associated information.

use crate::{
    Multiaddr,
    connection::{
        ConnectionId,
        ConnectedPoint,
        ConnectionError,
        ConnectionHandler,
        ConnectionInfo,
        ConnectionLimit,
        Connected,
        EstablishedConnection,
        IncomingInfo,
        IntoConnectionHandler,
        ListenerId,
        PendingConnectionError,
        Substream,
        pool::Pool,
    },
    muxing::StreamMuxer,
    transport::{Transport, TransportError},
};
use futures::prelude::*;
use std::{error, fmt, hash::Hash, num::NonZeroU32};

/// Event that can happen on the `Network`.
pub enum NetworkEvent<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
    /// One of the listeners gracefully closed.
    ListenerClosed {
        /// The listener ID that closed.
        listener_id: ListenerId,
        /// The addresses that the listener was listening on.
        addresses: Vec<Multiaddr>,
        /// Reason for the closure. Contains `Ok(())` if the stream produced `None`, or `Err`
        /// if the stream produced an error.
        reason: Result<(), TTrans::Error>,
    },

    /// One of the listeners reported a non-fatal error.
    ListenerError {
        /// The listener that errored.
        listener_id: ListenerId,
        /// The listener error.
        error: TTrans::Error
    },

    /// One of the listeners is now listening on an additional address.
    NewListenerAddress {
        /// The listener that is listening on the new address.
        listener_id: ListenerId,
        /// The new address the listener is now also listening on.
        listen_addr: Multiaddr
    },

    /// One of the listeners is no longer listening on some address.
    ExpiredListenerAddress {
        /// The listener that is no longer listening on some address.
        listener_id: ListenerId,
        /// The expired address.
        listen_addr: Multiaddr
    },

    /// A new connection arrived on a listener.
    IncomingConnection(IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>),

    /// An error happened on a connection during its initial handshake.
    ///
    /// This can include, for example, an error during the handshake of the encryption layer, or
    /// the connection unexpectedly closed.
    IncomingConnectionError {
        /// Local connection address.
        local_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
        /// The error that happened.
        error: PendingConnectionError<TTrans::Error>,
    },

    /// A new connection to a peer has been opened.
    ConnectionEstablished {
        /// The newly established connection.
        connection: EstablishedConnection<'a, TInEvent, TConnInfo, TPeerId>,
        /// The total number of established connections to the same peer, including the one that
        /// has just been opened.
        num_established: NonZeroU32,
    },

    /// An established connection to a peer has encountered an error.
    ///
    /// The connection is closed as a result of the error.
    ConnectionError {
        /// Information about the connection that encountered the error.
        connected: Connected<TConnInfo>,
        /// The error that occurred.
        error: ConnectionError<<THandler::Handler as ConnectionHandler>::Error>,
        /// The remaining number of established connections to the same peer.
        num_established: u32,
    },

    /// A dialing attempt to an address of a peer failed.
    DialError {
        /// The number of remaining dialing attempts.
        attempts_remaining: u32,

        /// Id of the peer we were trying to dial.
        peer_id: TPeerId,

        /// The multiaddr we failed to reach.
        multiaddr: Multiaddr,

        /// The error that happened.
        error: PendingConnectionError<TTrans::Error>,
    },

    /// Failed to reach a peer that we were trying to dial.
    UnknownPeerDialError {
        /// The multiaddr we failed to reach.
        multiaddr: Multiaddr,

        /// The error that happened.
        error: PendingConnectionError<TTrans::Error>,

        /// The handler that was passed to `dial()`, if the
        /// connection failed before the handler was consumed.
        handler: THandler,
    },

    /// An established connection produced an event.
    ConnectionEvent {
        /// The connection on which the event occurred.
        connection: EstablishedConnection<'a, TInEvent, TConnInfo, TPeerId>,
        /// Event that was produced by the node.
        event: TOutEvent,
    },
}

impl<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> fmt::Debug for
    NetworkEvent<'_, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TInEvent: fmt::Debug,
    TOutEvent: fmt::Debug,
    TTrans: Transport,
    TTrans::Error: fmt::Debug,
    THandler: IntoConnectionHandler<TConnInfo>,
    <THandler::Handler as ConnectionHandler>::Error: fmt::Debug,
    TConnInfo: fmt::Debug,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            NetworkEvent::NewListenerAddress { listener_id, listen_addr } => {
                f.debug_struct("NewListenerAddress")
                    .field("listener_id", listener_id)
                    .field("listen_addr", listen_addr)
                    .finish()
            }
            NetworkEvent::ExpiredListenerAddress { listener_id, listen_addr } => {
                f.debug_struct("ExpiredListenerAddress")
                    .field("listener_id", listener_id)
                    .field("listen_addr", listen_addr)
                    .finish()
            }
            NetworkEvent::ListenerClosed { listener_id, addresses, reason } => {
                f.debug_struct("ListenerClosed")
                    .field("listener_id", listener_id)
                    .field("addresses", addresses)
                    .field("reason", reason)
                    .finish()
            }
            NetworkEvent::ListenerError { listener_id, error } => {
                f.debug_struct("ListenerError")
                    .field("listener_id", listener_id)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::IncomingConnection(event) => {
                f.debug_struct("IncomingConnection")
                    .field("local_addr", &event.local_addr)
                    .field("send_back_addr", &event.send_back_addr)
                    .finish()
            }
            NetworkEvent::IncomingConnectionError { local_addr, send_back_addr, error } => {
                f.debug_struct("IncomingConnectionError")
                    .field("local_addr", local_addr)
                    .field("send_back_addr", send_back_addr)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::ConnectionEstablished { connection, .. } => {
                f.debug_struct("ConnectionEstablished")
                    .field("connection", connection)
                    .finish()
            }
            NetworkEvent::ConnectionError { connected, error, .. } => {
                f.debug_struct("ConnectionError")
                    .field("connected", connected)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::DialError { attempts_remaining, peer_id, multiaddr, error } => {
                f.debug_struct("DialError")
                    .field("attempts_remaining", attempts_remaining)
                    .field("peer_id", peer_id)
                    .field("multiaddr", multiaddr)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::UnknownPeerDialError { multiaddr, error, .. } => {
                f.debug_struct("UnknownPeerDialError")
                    .field("multiaddr", multiaddr)
                    .field("error", error)
                    .finish()
            }
            NetworkEvent::ConnectionEvent { connection, event } => {
                f.debug_struct("ConnectionEvent")
                    .field("connection", connection)
                    .field("event", event)
                    .finish()
            }
        }
    }
}

/// A new connection arrived on a listener.
pub struct IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
    /// The listener who received the connection.
    pub(super) listener_id: ListenerId,
    /// The produced upgrade.
    pub(super) upgrade: TTrans::ListenerUpgrade,
    /// Local connection address.
    pub(super) local_addr: Multiaddr,
    /// Address used to send back data to the remote.
    pub(super) send_back_addr: Multiaddr,
    /// Reference to the `peers` field of the `Network`.
    pub(super) pool: &'a mut Pool<
        TInEvent,
        TOutEvent,
        THandler,
        TTrans::Error,
        <THandler::Handler as ConnectionHandler>::Error,
        TConnInfo,
        TPeerId
    >,
}

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler, TConnInfo, TPeerId>
    IncomingConnectionEvent<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport<Output = (TConnInfo, TMuxer)>,
    TTrans::Error: Send + 'static,
    TTrans::ListenerUpgrade: Send + 'static,
    THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
    THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
    <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    <THandler::Handler as ConnectionHandler>::Error: error::Error + Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone + Send + 'static,
{
    /// The ID of the listener with the incoming connection.
    pub fn listener_id(&self) -> ListenerId {
        self.listener_id
    }

    /// Starts processing the incoming connection and sets the handler to use for it.
    pub fn accept(self, handler: THandler) -> Result<ConnectionId, ConnectionLimit> {
        self.accept_with_builder(|_| handler)
    }

    /// Same as `accept`, but accepts a closure that turns a `IncomingInfo` into a handler.
    pub fn accept_with_builder<TBuilder>(self, builder: TBuilder)
        -> Result<ConnectionId, ConnectionLimit>
    where
        TBuilder: FnOnce(IncomingInfo<'_>) -> THandler
    {
        let handler = builder(self.info());
        let upgrade = self.upgrade
            .map_err(|err| PendingConnectionError::Transport(TransportError::Other(err)));
        let info = IncomingInfo {
            local_addr: &self.local_addr,
            send_back_addr: &self.send_back_addr,
        };
        self.pool.add_incoming(upgrade, handler, info)
    }
}

impl<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    IncomingConnectionEvent<'_, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
    /// Returns the `IncomingInfo` corresponding to this incoming connection.
    pub fn info(&self) -> IncomingInfo<'_> {
        IncomingInfo {
            local_addr: &self.local_addr,
            send_back_addr: &self.send_back_addr,
        }
    }

    /// Local connection address.
    pub fn local_addr(&self) -> &Multiaddr {
        &self.local_addr
    }

    /// Address used to send back data to the dialer.
    pub fn send_back_addr(&self) -> &Multiaddr {
        &self.send_back_addr
    }

    /// Builds the `ConnectedPoint` corresponding to the incoming connection.
    pub fn to_connected_point(&self) -> ConnectedPoint {
        self.info().to_connected_point()
    }
}
