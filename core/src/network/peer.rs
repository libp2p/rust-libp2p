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

use super::Network;
use crate::{
    connection::{
        handler::THandlerInEvent, pool::Pool, ConnectionHandler, ConnectionId,
        EstablishedConnection, EstablishedConnectionIter, IntoConnectionHandler, PendingConnection,
    },
    PeerId, Transport,
};
use std::{collections::VecDeque, error, fmt};

/// The possible representations of a peer in a [`Network`], as
/// seen by the local node.
///
/// > **Note**: In any state there may always be a pending incoming
/// > connection attempt from the peer, however, the remote identity
/// > of a peer is only known once a connection is fully established.
pub enum Peer<'a, TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    /// At least one established connection exists to the peer.
    Connected(ConnectedPeer<'a, TTrans, THandler>),

    /// There is an ongoing dialing (i.e. outgoing connection) attempt
    /// to the peer. There may already be other established connections
    /// to the peer.
    Dialing(DialingPeer<'a, TTrans, THandler>),

    /// There exists no established connection to the peer and there is
    /// currently no ongoing dialing (i.e. outgoing connection) attempt
    /// in progress.
    Disconnected(DisconnectedPeer<'a, TTrans, THandler>),

    /// The peer represents the local node.
    Local,
}

impl<'a, TTrans, THandler> fmt::Debug for Peer<'a, TTrans, THandler>
where
    TTrans: Transport,
    TTrans::Error: Send + 'static,
    THandler: IntoConnectionHandler,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Peer::Connected(p) => f.debug_struct("Connected").field("peer", &p).finish(),
            Peer::Dialing(p) => f.debug_struct("Dialing").field("peer", &p).finish(),
            Peer::Disconnected(p) => f.debug_struct("Disconnected").field("peer", &p).finish(),
            Peer::Local => f.debug_struct("Local").finish(),
        }
    }
}

impl<'a, TTrans, THandler> Peer<'a, TTrans, THandler>
where
    TTrans: Transport,
    TTrans::Error: Send + 'static,
    THandler: IntoConnectionHandler,
{
    pub(super) fn new(network: &'a mut Network<TTrans, THandler>, peer_id: PeerId) -> Self {
        if peer_id == network.local_peer_id {
            return Peer::Local;
        }

        if network.pool.is_connected(&peer_id) {
            return Self::connected(network, peer_id);
        }

        if network.is_dialing(&peer_id) {
            return Self::dialing(network, peer_id);
        }

        Self::disconnected(network, peer_id)
    }

    fn disconnected(network: &'a mut Network<TTrans, THandler>, peer_id: PeerId) -> Self {
        Peer::Disconnected(DisconnectedPeer {
            _network: network,
            peer_id,
        })
    }

    fn connected(network: &'a mut Network<TTrans, THandler>, peer_id: PeerId) -> Self {
        Peer::Connected(ConnectedPeer { network, peer_id })
    }

    fn dialing(network: &'a mut Network<TTrans, THandler>, peer_id: PeerId) -> Self {
        Peer::Dialing(DialingPeer { network, peer_id })
    }
}

impl<'a, TTrans, THandler> Peer<'a, TTrans, THandler>
where
    TTrans: Transport + Clone + Send + 'static,
    TTrans::Output: Send + 'static,
    TTrans::Error: Send + 'static,
    TTrans::Dial: Send + 'static,
    THandler: IntoConnectionHandler + Send + 'static,
    THandler::Handler: ConnectionHandler + Send,
    <THandler::Handler as ConnectionHandler>::OutboundOpenInfo: Send,
    <THandler::Handler as ConnectionHandler>::Error: error::Error + Send + 'static,
{
    /// Checks whether the peer is currently connected.
    ///
    /// Returns `true` iff [`Peer::into_connected`] returns `Some`.
    pub fn is_connected(&self) -> bool {
        match self {
            Peer::Connected(..) => true,
            Peer::Dialing(peer) => peer.is_connected(),
            Peer::Disconnected(..) => false,
            Peer::Local => false,
        }
    }

    /// Checks whether the peer is currently being dialed.
    ///
    /// Returns `true` iff [`Peer::into_dialing`] returns `Some`.
    pub fn is_dialing(&self) -> bool {
        match self {
            Peer::Dialing(_) => true,
            Peer::Connected(peer) => peer.is_dialing(),
            Peer::Disconnected(..) => false,
            Peer::Local => false,
        }
    }

    /// Checks whether the peer is currently disconnected.
    ///
    /// Returns `true` iff [`Peer::into_disconnected`] returns `Some`.
    pub fn is_disconnected(&self) -> bool {
        matches!(self, Peer::Disconnected(..))
    }

    /// Converts the peer into a `ConnectedPeer`, if an established connection exists.
    ///
    /// Succeeds if the there is at least one established connection to the peer.
    pub fn into_connected(self) -> Option<ConnectedPeer<'a, TTrans, THandler>> {
        match self {
            Peer::Connected(peer) => Some(peer),
            Peer::Dialing(peer) => peer.into_connected(),
            Peer::Disconnected(..) => None,
            Peer::Local => None,
        }
    }

    /// Converts the peer into a `DialingPeer`, if a dialing attempt exists.
    ///
    /// Succeeds if the there is at least one pending outgoing connection to the peer.
    pub fn into_dialing(self) -> Option<DialingPeer<'a, TTrans, THandler>> {
        match self {
            Peer::Dialing(peer) => Some(peer),
            Peer::Connected(peer) => peer.into_dialing(),
            Peer::Disconnected(..) => None,
            Peer::Local => None,
        }
    }

    /// Converts the peer into a `DisconnectedPeer`, if neither an established connection
    /// nor a dialing attempt exists.
    pub fn into_disconnected(self) -> Option<DisconnectedPeer<'a, TTrans, THandler>> {
        match self {
            Peer::Disconnected(peer) => Some(peer),
            _ => None,
        }
    }
}

/// The representation of a peer in a [`Network`] to whom at least
/// one established connection exists. There may also be additional ongoing
/// dialing attempts to the peer.
pub struct ConnectedPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    network: &'a mut Network<TTrans, THandler>,
    peer_id: PeerId,
}

impl<'a, TTrans, THandler> ConnectedPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    <TTrans as Transport>::Error: Send + 'static,
    THandler: IntoConnectionHandler,
{
    pub fn id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the `ConnectedPeer` into a `Peer`.
    pub fn into_peer(self) -> Peer<'a, TTrans, THandler> {
        Peer::Connected(self)
    }

    /// Obtains an established connection to the peer by ID.
    pub fn connection(
        &mut self,
        id: ConnectionId,
    ) -> Option<EstablishedConnection<THandlerInEvent<THandler>>> {
        self.network.pool.get_established(id)
    }

    /// The number of established connections to the peer.
    pub fn num_connections(&self) -> u32 {
        self.network.pool.num_peer_established(self.peer_id)
    }

    /// Checks whether there is an ongoing dialing attempt to the peer.
    ///
    /// Returns `true` iff [`ConnectedPeer::into_dialing`] returns `Some`.
    pub fn is_dialing(&self) -> bool {
        self.network.is_dialing(&self.peer_id)
    }

    /// Converts this peer into a [`DialingPeer`], if there is an ongoing
    /// dialing attempt, `None` otherwise.
    pub fn into_dialing(self) -> Option<DialingPeer<'a, TTrans, THandler>> {
        if self.network.is_dialing(&self.peer_id) {
            Some(DialingPeer {
                network: self.network,
                peer_id: self.peer_id,
            })
        } else {
            None
        }
    }

    /// Gets an iterator over all established connections to the peer.
    pub fn connections(
        &mut self,
    ) -> EstablishedConnectionIter<impl Iterator<Item = ConnectionId>, THandlerInEvent<THandler>>
    {
        self.network.pool.iter_peer_established(&self.peer_id)
    }

    /// Obtains some established connection to the peer.
    pub fn some_connection(&mut self) -> EstablishedConnection<THandlerInEvent<THandler>> {
        self.connections()
            .into_first()
            .expect("By `Peer::new` and the definition of `ConnectedPeer`.")
    }

    /// Disconnects from the peer, closing all connections.
    pub fn disconnect(self) -> DisconnectedPeer<'a, TTrans, THandler> {
        self.network.disconnect(&self.peer_id);
        DisconnectedPeer {
            _network: self.network,
            peer_id: self.peer_id,
        }
    }
}

impl<'a, TTrans, THandler> fmt::Debug for ConnectedPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    TTrans::Error: Send + 'static,
    THandler: IntoConnectionHandler,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ConnectedPeer")
            .field("peer_id", &self.peer_id)
            .field(
                "established",
                &self
                    .network
                    .pool
                    .iter_peer_established_info(&self.peer_id)
                    .collect::<Vec<_>>(),
            )
            .field("attempts", &self.network.is_dialing(&self.peer_id))
            .finish()
    }
}

/// The representation of a peer in a [`Network`] to whom a dialing
/// attempt is ongoing. There may already exist other established
/// connections to this peer.
pub struct DialingPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    network: &'a mut Network<TTrans, THandler>,
    peer_id: PeerId,
}

impl<'a, TTrans, THandler> DialingPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    TTrans::Error: Send + 'static,
    THandler: IntoConnectionHandler,
{
    pub fn id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the `DialingPeer` into a `Peer`.
    pub fn into_peer(self) -> Peer<'a, TTrans, THandler> {
        Peer::Dialing(self)
    }

    /// Disconnects from this peer, closing all established connections and
    /// aborting all dialing attempts.
    pub fn disconnect(self) -> DisconnectedPeer<'a, TTrans, THandler> {
        self.network.disconnect(&self.peer_id);
        DisconnectedPeer {
            _network: self.network,
            peer_id: self.peer_id,
        }
    }

    /// Checks whether there is an established connection to the peer.
    ///
    /// Returns `true` iff [`DialingPeer::into_connected`] returns `Some`.
    pub fn is_connected(&self) -> bool {
        self.network.pool.is_connected(&self.peer_id)
    }

    /// Converts the peer into a `ConnectedPeer`, if an established connection exists.
    pub fn into_connected(self) -> Option<ConnectedPeer<'a, TTrans, THandler>> {
        if self.is_connected() {
            Some(ConnectedPeer {
                peer_id: self.peer_id,
                network: self.network,
            })
        } else {
            None
        }
    }

    /// Obtains a dialing attempt to the peer by connection ID of
    /// the current connection attempt.
    pub fn attempt(&mut self, id: ConnectionId) -> Option<DialingAttempt<'_, THandler>> {
        Some(DialingAttempt {
            peer_id: self.peer_id,
            inner: self.network.pool.get_outgoing(id)?,
        })
    }

    /// Gets an iterator over all dialing (i.e. pending outgoing) connections to the peer.
    pub fn attempts(&mut self) -> DialingAttemptIter<'_, THandler, TTrans> {
        DialingAttemptIter::new(&self.peer_id, &mut self.network)
    }

    /// Obtains some dialing connection to the peer.
    ///
    /// At least one dialing connection is guaranteed to exist on a `DialingPeer`.
    pub fn some_attempt(&mut self) -> DialingAttempt<'_, THandler> {
        self.attempts()
            .into_first()
            .expect("By `Peer::new` and the definition of `DialingPeer`.")
    }
}

impl<'a, TTrans, THandler> fmt::Debug for DialingPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    TTrans::Error: Send + 'static,
    THandler: IntoConnectionHandler,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("DialingPeer")
            .field("peer_id", &self.peer_id)
            .field(
                "established",
                &self
                    .network
                    .pool
                    .iter_peer_established_info(&self.peer_id)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// The representation of a peer to whom the `Network` has currently
/// neither an established connection, nor an ongoing dialing attempt
/// initiated by the local peer.
pub struct DisconnectedPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    peer_id: PeerId,
    _network: &'a mut Network<TTrans, THandler>,
}

impl<'a, TTrans, THandler> fmt::Debug for DisconnectedPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("DisconnectedPeer")
            .field("peer_id", &self.peer_id)
            .finish()
    }
}

impl<'a, TTrans, THandler> DisconnectedPeer<'a, TTrans, THandler>
where
    TTrans: Transport,
    THandler: IntoConnectionHandler,
{
    pub fn id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the `DisconnectedPeer` into a `Peer`.
    pub fn into_peer(self) -> Peer<'a, TTrans, THandler> {
        Peer::Disconnected(self)
    }
}

/// A [`DialingAttempt`] is a pending outgoing connection attempt to a known /
/// expected remote peer ID.
pub struct DialingAttempt<'a, THandler: IntoConnectionHandler> {
    peer_id: PeerId,
    /// The underlying pending connection in the `Pool`.
    inner: PendingConnection<'a, THandler>,
}

impl<'a, THandler: IntoConnectionHandler> DialingAttempt<'a, THandler> {
    /// Returns the ID of the current connection attempt.
    pub fn id(&self) -> ConnectionId {
        self.inner.id()
    }

    /// Returns the (expected) peer ID of the dialing attempt.
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Aborts the dialing attempt.
    pub fn abort(self) {
        self.inner.abort();
    }
}

/// An iterator over the ongoing dialing attempts to a peer.
pub struct DialingAttemptIter<'a, THandler: IntoConnectionHandler, TTrans: Transport> {
    /// The peer whose dialing attempts are being iterated.
    peer_id: &'a PeerId,
    /// The underlying connection `Pool` of the `Network`.
    pool: &'a mut Pool<THandler, TTrans>,
    /// [`ConnectionId`]s of the dialing attempts of the peer.
    connections: VecDeque<ConnectionId>,
}

// Note: Ideally this would be an implementation of `Iterator`, but that
// requires GATs (cf. https://github.com/rust-lang/rust/issues/44265) and
// a different definition of `Iterator`.
impl<'a, THandler: IntoConnectionHandler, TTrans: Transport>
    DialingAttemptIter<'a, THandler, TTrans>
{
    fn new(peer_id: &'a PeerId, network: &'a mut Network<TTrans, THandler>) -> Self {
        let connections = network.dialing_attempts(*peer_id).map(|id| *id).collect();
        Self {
            pool: &mut network.pool,
            peer_id,
            connections,
        }
    }

    /// Obtains the next dialing connection, if any.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<DialingAttempt<'_, THandler>> {
        let connection_id = self.connections.pop_front()?;

        let inner = self.pool.get_outgoing(connection_id)?;

        Some(DialingAttempt {
            peer_id: *self.peer_id,
            inner,
        })
    }

    /// Returns the first connection, if any, consuming the iterator.
    pub fn into_first<'b>(mut self) -> Option<DialingAttempt<'b, THandler>>
    where
        'a: 'b,
    {
        let connection_id = self.connections.pop_front()?;

        let inner = self.pool.get_outgoing(connection_id)?;

        Some(DialingAttempt {
            peer_id: *self.peer_id,
            inner,
        })
    }
}
