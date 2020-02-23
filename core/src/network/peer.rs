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

use crate::{
    Multiaddr,
    Transport,
    StreamMuxer,
    connection::{
        Connected,
        ConnectedPoint,
        ConnectionHandler,
        ConnectionInfo,
        Connection,
        ConnectionId,
        ConnectionLimit,
        EstablishedConnection,
        EstablishedConnectionIter,
        IntoConnectionHandler,
        PendingConnection,
        Substream,
    },
};
use std::{
    collections::hash_map,
    error,
    fmt,
    hash::Hash,
    num::NonZeroUsize,
};
use super::{Network, DialingOpts};

/// The state of a (remote) peer as seen by the local peer
/// through a [`Network`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerState {
    /// The [`Network`] is connected to the peer, i.e. has at least one
    /// established connection.
    Connected,
    /// We are currently trying to reach this peer.
    Dialing {
        /// Number of addresses we are trying to dial.
        num_pending_addresses: NonZeroUsize,
    },
    /// The [`Network`] is disconnected from the peer, i.e. has no
    /// established connection and no pending, outgoing connection.
    Disconnected,
}

/// The possible representations of a peer in a [`Network`], as
/// seen by the local node.
pub enum Peer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>
{
    /// At least one established connection exists to the peer.
    Connected(ConnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>),

    /// There is an ongoing dialing (i.e. outgoing connection) attempt
    /// to the peer. There may already be other established connections
    /// to the peer.
    Dialing(DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>),

    /// There exists no established connection to the peer and there is
    /// currently no ongoing dialing (i.e. outgoing connection) attempt
    /// in progress.
    ///
    /// > **Note**: In this state there may always be a pending incoming
    /// > connection attempt from the peer, however, the remote identity
    /// > of a peer is only known once a connection is fully established.
    Disconnected(DisconnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>),

    /// The peer represents the local node.
    Local,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> fmt::Debug for
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId>,
    TPeerId: fmt::Debug + Eq + Hash,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match *self {
            Peer::Connected(ConnectedPeer { ref peer_id, .. }) => {
                f.debug_struct("Connected")
                    .field("peer_id", peer_id)
                    .finish()
            }
            Peer::Dialing(DialingPeer { ref peer_id, .. } ) => {
                f.debug_struct("DialingPeer")
                    .field("peer_id", peer_id)
                    .finish()
            }
            Peer::Disconnected(DisconnectedPeer { ref peer_id, .. }) => {
                f.debug_struct("Disconnected")
                    .field("peer_id", peer_id)
                    .finish()
            }
            Peer::Local => {
                f.debug_struct("Local")
                    .finish()
            }
        }
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TPeerId: Eq + Hash,
    TConnInfo: ConnectionInfo<PeerId = TPeerId>
{
    pub(super) fn new(
        network: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
        peer_id: TPeerId
    ) -> Self {
        if peer_id == network.local_peer_id {
            return Peer::Local;
        }

        if network.pool.is_connected(&peer_id) {
            return Self::connected(network, peer_id)
        }

        if network.dialing.get_mut(&peer_id).is_some() {
            return Self::dialing(network, peer_id);
        }

        Self::disconnected(network, peer_id)
    }


    fn disconnected(
        network: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
        peer_id: TPeerId
    ) -> Self {
        Peer::Disconnected(DisconnectedPeer { network, peer_id })
    }

    fn connected(
        network: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
        peer_id: TPeerId
    ) -> Self {
        Peer::Connected(ConnectedPeer { network, peer_id })
    }

    fn dialing(
        network: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
        peer_id: TPeerId
    ) -> Self {
        Peer::Dialing(DialingPeer { network, peer_id })
    }
}

impl<'a, TTrans, TMuxer, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    Peer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport<Output = (TConnInfo, TMuxer)> + Clone,
    TTrans::Error: Send + 'static,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
    THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
    <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send + 'static, // TODO: shouldn't be necessary
    <THandler::Handler as ConnectionHandler>::Error: error::Error + Send + 'static,
    TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
    TPeerId: Eq + Hash + Clone + Send + 'static,
{

    /// If we are connected, returns the `ConnectedPeer`.
    pub fn into_connected(self) -> Option<
        ConnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    > {
        match self {
            Peer::Connected(peer) => Some(peer),
            _ => None,
        }
    }

    /// If a connection is pending, returns the `DialingPeer`.
    pub fn into_dialing(self) -> Option<
        DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    > {
        match self {
            Peer::Dialing(peer) => Some(peer),
            _ => None,
        }
    }

    /// If we are not connected, returns the `DisconnectedPeer`.
    pub fn into_disconnected(self) -> Option<
        DisconnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    > {
        match self {
            Peer::Disconnected(peer) => Some(peer),
            _ => None,
        }
    }
}

/// The representation of a peer in a [`Network`] to whom at least
/// one established connection exists.
pub struct ConnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
    network: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
    peer_id: TPeerId,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    ConnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    TPeerId: Eq + Hash + Clone,
{
    /// Attempts to establish a new connection to this peer using the given addresses,
    /// if there is currently no ongoing dialing attempt.
    ///
    /// Existing established connections are not affected.
    ///
    /// > **Note**: If there is an ongoing dialing attempt, a `DialingPeer`
    /// > is returned with the given addresses and handler being ignored.
    /// > You may want to check [`ConnectedPeer::is_dialing`] first.
    pub fn connect<I, TMuxer>(self, address: Multiaddr, remaining: I, handler: THandler)
        -> Result<DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
                  ConnectionLimit>
    where
        I: IntoIterator<Item = Multiaddr>,
        THandler: Send + 'static,
        THandler::Handler: Send,
        <THandler::Handler as ConnectionHandler>::Error: error::Error + Send,
        <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send,
        THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send,
        TTrans: Transport<Output = (TConnInfo, TMuxer)> + Clone,
        TTrans::Error: Send + 'static,
        TTrans::Dial: Send + 'static,
        TMuxer: StreamMuxer + Send + Sync + 'static,
        TMuxer::OutboundSubstream: Send,
        TMuxer::Substream: Send,
        TConnInfo: fmt::Debug + Send + 'static,
        TPeerId: Eq + Hash + Clone + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,

    {
        if self.network.dialing.contains_key(&self.peer_id) {
            let peer = DialingPeer {
                network: self.network,
                peer_id: self.peer_id
            };
            Ok(peer)
        } else {
           self.network.dial_peer(DialingOpts {
                peer: self.peer_id.clone(),
                handler,
                address,
                remaining: remaining.into_iter().collect(),
            })?;
            Ok(DialingPeer {
                network: self.network,
                peer_id: self.peer_id,
            })
        }
    }

    /// Obtains an existing connection to the peer.
    pub fn connection<'b>(&'b mut self, id: ConnectionId)
        -> Option<EstablishedConnection<'b, TInEvent, TConnInfo, TPeerId>>
    {
        self.network.pool.get_established(id)
    }

    /// The number of established connections to the peer.
    pub fn num_connections(&self) -> usize {
        self.network.pool.num_peer_established(&self.peer_id)
    }

    /// Checks whether there is an ongoing dialing attempt to the peer.
    ///
    /// Returns `true` iff [`ConnectedPeer::into_dialing`] returns `Some`.
    pub fn is_dialing(&self) -> bool {
        self.network.dialing.contains_key(&self.peer_id)
    }

    /// Turns this peer into a [`DialingPeer`], if there is an ongoing
    /// dialing attempt, `None` otherwise.
    pub fn into_dialing(self) -> Option<
        DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    > {
        if self.network.dialing.contains_key(&self.peer_id) {
            Some(DialingPeer { network: self.network, peer_id: self.peer_id })
        } else {
            None
        }
    }

    /// Gets an iterator over all established connections of the peer.
    pub fn connections<'b>(&'b mut self) ->
        EstablishedConnectionIter<'b,
            impl Iterator<Item = ConnectionId>,
            TInEvent,
            TOutEvent,
            THandler,
            TTrans::Error,
            <THandler::Handler as ConnectionHandler>::Error,
            TConnInfo,
            TPeerId>
    {
        self.network.pool.iter_peer_established(&self.peer_id)
    }

    /// Obtains some established connection to the peer.
    pub fn some_connection<'b>(&'b mut self)
        -> EstablishedConnection<'b, TInEvent, TConnInfo, TPeerId>
    {
        self.connections()
            .into_first()
            .expect("By `Peer::new` and the definition of `ConnectedPeer`.")
    }

    /// Disconnects from the peer, closing all connections.
    pub fn disconnect(self)
        -> DisconnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    {
        self.network.disconnect(&self.peer_id);
        DisconnectedPeer { network: self.network, peer_id: self.peer_id }
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> fmt::Debug for
    ConnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ConnectedPeer")
            .field("peer_id", &self.peer_id)
            .finish()
    }
}

/// The representation of a peer in a [`Network`] to whom a dialing
/// attempt is ongoing. There may already exist other established
/// connections to this peer.
pub struct DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
    network: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
    peer_id: TPeerId,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
    DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TConnInfo: ConnectionInfo<PeerId = TPeerId>,
    TPeerId: Eq + Hash + Clone,
{
    /// Disconnects from this peer, closing all pending connections.
    pub fn disconnect(self) -> DisconnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> {
        self.network.disconnect(&self.peer_id);
        DisconnectedPeer { network: self.network, peer_id: self.peer_id }
    }

    /// Obtains the connection that is currently being established.
    pub fn connection<'b>(&'b mut self) -> DialingConnection<'b, TInEvent, TConnInfo, TPeerId> {
        let attempt = match self.network.dialing.entry(self.peer_id.clone()) {
            hash_map::Entry::Occupied(e) => e,
            _ => unreachable!("By `Peer::new` and the definition of `DialingPeer`.")
        };

        let inner = self.network.pool
            .get_outgoing(attempt.get().id)
            .expect("By consistency of `network.pool` with `network.dialing`.");

        DialingConnection {
            inner, dialing: attempt, peer_id: &self.peer_id
        }
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> fmt::Debug for
    DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("DialingPeer")
            .field("peer_id", &self.peer_id)
            .finish()
    }
}

/// The representation of a peer to whom the `Network` has currently
/// neither an established connection, nor an ongoing dialing attempt
/// initiated by the local peer.
pub struct DisconnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
{
    peer_id: TPeerId,
    network: &'a mut Network<TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
}

impl<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId> fmt::Debug for
    DisconnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler<TConnInfo>,
    TPeerId: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("DisconnectedPeer")
            .field("peer_id", &self.peer_id)
            .finish()
    }
}

impl<'a, TTrans, TInEvent, TOutEvent, TMuxer, THandler, TConnInfo, TPeerId>
    DisconnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>
where
    TTrans: Transport<Output = (TConnInfo, TMuxer)> + Clone,
    TTrans::Error: Send + 'static,
    TTrans::Dial: Send + 'static,
    TMuxer: StreamMuxer + Send + Sync + 'static,
    TMuxer::OutboundSubstream: Send,
    TMuxer::Substream: Send,
    THandler: IntoConnectionHandler<TConnInfo> + Send + 'static,
    THandler::Handler: ConnectionHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send,
    <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send,
    <THandler::Handler as ConnectionHandler>::Error: error::Error + Send,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
{
    /// Attempts to connect to this peer using the given addresses.
    pub fn connect<TIter>(self, first: Multiaddr, rest: TIter, handler: THandler)
        -> Result<DialingPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
                  ConnectionLimit>
    where
        TIter: IntoIterator<Item = Multiaddr>,
        TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Send + 'static,
        TPeerId: Eq + Hash + Clone + Send + 'static,
    {
        self.network.dial_peer(DialingOpts {
            peer: self.peer_id.clone(),
            handler,
            address: first,
            remaining: rest.into_iter().collect(),
        })?;
        Ok(DialingPeer {
            network: self.network,
            peer_id: self.peer_id,
        })

    }

    /// Moves the peer into a connected state by supplying an existing
    /// established connection.
    ///
    /// No event is generated for this action.
    ///
    /// # Panics
    ///
    /// Panics if `connected.peer_id()` does not identify the current peer.
    ///
    pub fn set_connected(
        self,
        connected: Connected<TConnInfo>,
        connection: Connection<TMuxer, THandler::Handler>,
    ) -> Result<
        ConnectedPeer<'a, TTrans, TInEvent, TOutEvent, THandler, TConnInfo, TPeerId>,
        ConnectionLimit
    > where
        TConnInfo: fmt::Debug + ConnectionInfo<PeerId = TPeerId> + Clone + Send + 'static,
        TPeerId: Eq + Hash + Clone + fmt::Debug,
    {
        if connected.peer_id() != &self.peer_id {
            panic!("Invalid peer ID given: {:?}. Expected: {:?}", connected.peer_id(), self.peer_id)
        }

        self.network.pool.add(connection, connected)
            .map(|_id| ConnectedPeer {
                network: self.network,
                peer_id: self.peer_id,
            })
    }
}

/// Attempt to reach a peer.
#[derive(Debug, Clone)]
pub(super) struct DialingAttempt {
    /// Identifier for the reach attempt.
    pub(super) id: ConnectionId,
    /// Multiaddr currently being attempted.
    pub(super) current: Multiaddr,
    /// Multiaddresses to attempt if the current one fails.
    pub(super) next: Vec<Multiaddr>,
}

/// A `DialingConnection` is a [`PendingConnection`] where the local peer
/// has the role of the dialer (i.e. initiator) and the (expected) remote
/// peer ID is known.
pub struct DialingConnection<'a, TInEvent, TConnInfo, TPeerId> {
    peer_id: &'a TPeerId,
    inner: PendingConnection<'a, TInEvent, TConnInfo, TPeerId>,
    dialing: hash_map::OccupiedEntry<'a, TPeerId, DialingAttempt>,
}

impl<'a, TInEvent, TConnInfo, TPeerId>
    DialingConnection<'a, TInEvent, TConnInfo, TPeerId>
{
    /// Returns the local connection ID.
    pub fn id(&self) -> ConnectionId {
        self.inner.id()
    }

    /// Returns the (expected) peer ID of the ongoing connection attempt.
    pub fn peer_id(&self) -> &TPeerId {
        self.peer_id
    }

    /// Returns information about this endpoint of the connection attempt.
    pub fn endpoint(&self) -> &ConnectedPoint {
        self.inner.endpoint()
    }

    /// Aborts the connection attempt.
    pub fn abort(self)
    where
        TPeerId: Eq + Hash + Clone,
    {
        self.dialing.remove();
        self.inner.abort();
    }

    /// Adds new candidate addresses to the end of the addresses used
    /// in the ongoing dialing process.
    ///
    /// Duplicates are ignored.
    pub fn add_addresses(&mut self, addrs: impl IntoIterator<Item = Multiaddr>) {
        for addr in addrs {
            self.add_address(addr);
        }
    }

    /// Adds an address to the end of the addresses used in the ongoing
    /// dialing process.
    ///
    /// Duplicates are ignored.
    pub fn add_address(&mut self, addr: Multiaddr) {
        if self.dialing.get().next.iter().all(|a| a != &addr) {
            self.dialing.get_mut().next.push(addr);
        }
    }
}

