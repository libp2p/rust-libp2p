// Copyright 2020 Parity Technologies (UK) Ltd.
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

use crate::behaviour::{
    ConnectionClosed, ConnectionEstablished, DialFailure, ExpiredExternalAddr, ExpiredListenAddr,
    FromSwarm, ListenerClosed, ListenerError, NewExternalAddr, NewListenAddr, NewListener,
};
use crate::{
    ConnectionHandler, ConnectionId, IntoConnectionHandler, NetworkBehaviour,
    NetworkBehaviourAction, PollParameters, THandlerInEvent, THandlerOutEvent,
};
use libp2p_core::{multiaddr::Multiaddr, transport::ListenerId, ConnectedPoint, PeerId};
use std::collections::HashMap;
use std::task::{Context, Poll};

/// A `MockBehaviour` is a `NetworkBehaviour` that allows for
/// the instrumentation of return values, without keeping
/// any further state.
pub struct MockBehaviour<THandler, TOutEvent>
where
    THandler: ConnectionHandler,
{
    /// The prototype protocols handler that is cloned for every
    /// invocation of `new_handler`.
    pub handler_proto: THandler,
    /// The addresses to return from `addresses_of_peer`.
    pub addresses: HashMap<PeerId, Vec<Multiaddr>>,
    /// The next action to return from `poll`.
    ///
    /// An action is only returned once.
    pub next_action: Option<NetworkBehaviourAction<TOutEvent, THandler::InEvent>>,
}

impl<THandler, TOutEvent> MockBehaviour<THandler, TOutEvent>
where
    THandler: ConnectionHandler,
{
    pub fn new(handler_proto: THandler) -> Self {
        MockBehaviour {
            handler_proto,
            addresses: HashMap::new(),
            next_action: None,
        }
    }
}

impl<THandler, TOutEvent> NetworkBehaviour for MockBehaviour<THandler, TOutEvent>
where
    THandler: ConnectionHandler + Clone,
    THandler::OutEvent: Clone,
    TOutEvent: Send + 'static,
{
    type ConnectionHandler = THandler;
    type OutEvent = TOutEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        self.handler_proto.clone()
    }

    fn addresses_of_peer(&mut self, p: &PeerId) -> Vec<Multiaddr> {
        self.addresses.get(p).map_or(Vec::new(), |v| v.clone())
    }

    fn poll(
        &mut self,
        _: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
        self.next_action.take().map_or(Poll::Pending, Poll::Ready)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(_)
            | FromSwarm::ConnectionClosed(_)
            | FromSwarm::AddressChange(_)
            | FromSwarm::DialFailure(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListener(_)
            | FromSwarm::NewListenAddr(_)
            | FromSwarm::ExpiredListenAddr(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddr(_)
            | FromSwarm::ExpiredExternalAddr(_) => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        _event: THandlerOutEvent<Self>,
    ) {
    }
}

/// A `CallTraceBehaviour` is a `NetworkBehaviour` that tracks
/// invocations of callback methods and their arguments, wrapping
/// around an inner behaviour. It ensures certain invariants are met.
pub struct CallTraceBehaviour<TInner>
where
    TInner: NetworkBehaviour,
{
    inner: TInner,

    pub addresses_of_peer: Vec<PeerId>,
    pub on_connection_established: Vec<(PeerId, ConnectionId, ConnectedPoint, usize)>,
    pub on_connection_closed: Vec<(PeerId, ConnectionId, ConnectedPoint, usize)>,
    pub on_connection_handler_event: Vec<(
        PeerId,
        ConnectionId,
        <<TInner::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
    )>,
    pub on_dial_failure: Vec<Option<PeerId>>,
    pub on_new_listener: Vec<ListenerId>,
    pub on_new_listen_addr: Vec<(ListenerId, Multiaddr)>,
    pub on_new_external_addr: Vec<Multiaddr>,
    pub on_expired_listen_addr: Vec<(ListenerId, Multiaddr)>,
    pub on_expired_external_addr: Vec<Multiaddr>,
    pub on_listener_error: Vec<ListenerId>,
    pub on_listener_closed: Vec<(ListenerId, bool)>,
    pub poll: usize,
}

impl<TInner> CallTraceBehaviour<TInner>
where
    TInner: NetworkBehaviour,
    <<TInner::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent:
        Clone,
{
    pub fn new(inner: TInner) -> Self {
        Self {
            inner,
            addresses_of_peer: Vec::new(),
            on_connection_established: Vec::new(),
            on_connection_closed: Vec::new(),
            on_connection_handler_event: Vec::new(),
            on_dial_failure: Vec::new(),
            on_new_listener: Vec::new(),
            on_new_listen_addr: Vec::new(),
            on_new_external_addr: Vec::new(),
            on_expired_listen_addr: Vec::new(),
            on_expired_external_addr: Vec::new(),
            on_listener_error: Vec::new(),
            on_listener_closed: Vec::new(),
            poll: 0,
        }
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.addresses_of_peer = Vec::new();
        self.on_connection_established = Vec::new();
        self.on_connection_closed = Vec::new();
        self.on_connection_handler_event = Vec::new();
        self.on_dial_failure = Vec::new();
        self.on_new_listen_addr = Vec::new();
        self.on_new_external_addr = Vec::new();
        self.on_expired_listen_addr = Vec::new();
        self.on_listener_error = Vec::new();
        self.on_listener_closed = Vec::new();
        self.poll = 0;
    }

    pub fn inner(&mut self) -> &mut TInner {
        &mut self.inner
    }

    pub fn num_connections_to_peer(&self, peer: PeerId) -> usize {
        self.on_connection_established
            .iter()
            .filter(|(peer_id, _, _, _)| *peer_id == peer)
            .count()
            - self
                .on_connection_closed
                .iter()
                .filter(|(peer_id, _, _, _)| *peer_id == peer)
                .count()
    }

    /// Checks that when the expected number of closed connection notifications are received, a
    /// given number of expected disconnections have been received as well.
    ///
    /// Returns if the first condition is met.
    pub fn assert_disconnected(
        &self,
        expected_closed_connections: usize,
        expected_disconnections: usize,
    ) -> bool {
        if self.on_connection_closed.len() == expected_closed_connections {
            assert_eq!(
                self.on_connection_closed
                    .iter()
                    .filter(|(.., remaining_established)| { *remaining_established == 0 })
                    .count(),
                expected_disconnections
            );
            return true;
        }

        false
    }

    /// Checks that when the expected number of established connection notifications are received,
    /// a given number of expected connections have been received as well.
    ///
    /// Returns if the first condition is met.
    pub fn assert_connected(
        &self,
        expected_established_connections: usize,
        expected_connections: usize,
    ) -> bool {
        if self.on_connection_established.len() == expected_established_connections {
            assert_eq!(
                self.on_connection_established
                    .iter()
                    .filter(|(.., reported_aditional_connections)| {
                        *reported_aditional_connections == 0
                    })
                    .count(),
                expected_connections
            );
            return true;
        }

        false
    }

    fn on_connection_established(
        &mut self,
        ConnectionEstablished {
            peer_id,
            connection_id,
            endpoint,
            failed_addresses,
            other_established,
        }: ConnectionEstablished,
    ) {
        let mut other_peer_connections = self
            .on_connection_established
            .iter()
            .rev() // take last to first
            .filter_map(|(peer, .., other_established)| {
                if &peer_id == peer {
                    Some(other_established)
                } else {
                    None
                }
            })
            .take(other_established);

        // We are informed that there are `other_established` additional connections. Ensure that the
        // number of previous connections is consistent with this
        if let Some(&prev) = other_peer_connections.next() {
            if prev < other_established {
                assert_eq!(
                    prev,
                    other_established - 1,
                    "Inconsistent connection reporting"
                )
            }
            assert_eq!(other_peer_connections.count(), other_established - 1);
        } else {
            assert_eq!(other_established, 0)
        }
        self.on_connection_established.push((
            peer_id,
            connection_id,
            endpoint.clone(),
            other_established,
        ));
        self.inner
            .on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                failed_addresses,
                other_established,
            }));
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            connection_id,
            endpoint,
            handler,
            remaining_established,
        }: ConnectionClosed<<Self as NetworkBehaviour>::ConnectionHandler>,
    ) {
        let mut other_closed_connections = self
            .on_connection_established
            .iter()
            .rev() // take last to first
            .filter_map(|(peer, .., remaining_established)| {
                if &peer_id == peer {
                    Some(remaining_established)
                } else {
                    None
                }
            })
            .take(remaining_established);

        // We are informed that there are `other_established` additional connections. Ensure that the
        // number of previous connections is consistent with this
        if let Some(&prev) = other_closed_connections.next() {
            if prev < remaining_established {
                assert_eq!(
                    prev,
                    remaining_established - 1,
                    "Inconsistent closed connection reporting"
                )
            }
            assert_eq!(other_closed_connections.count(), remaining_established - 1);
        } else {
            assert_eq!(remaining_established, 0)
        }
        assert!(
            self.on_connection_established
                .iter()
                .any(|(peer, conn_id, endpoint, _)| (peer, conn_id, endpoint)
                    == (&peer_id, &connection_id, endpoint)),
            "`on_swarm_event` with `FromSwarm::ConnectionClosed is called only for connections for\
             which `on_swarm_event` with `FromSwarm::ConnectionEstablished` was called first."
        );
        self.on_connection_closed.push((
            peer_id,
            connection_id,
            endpoint.clone(),
            remaining_established,
        ));
        self.inner
            .on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                endpoint,
                handler,
                remaining_established,
            }));
    }
}

impl<TInner> NetworkBehaviour for CallTraceBehaviour<TInner>
where
    TInner: NetworkBehaviour,
    <<TInner::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent:
        Clone,
{
    type ConnectionHandler = TInner::ConnectionHandler;
    type OutEvent = TInner::OutEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, p: &PeerId) -> Vec<Multiaddr> {
        self.addresses_of_peer.push(*p);
        self.inner.addresses_of_peer(p)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                self.on_connection_established(connection_established)
            }
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.on_connection_closed(connection_closed)
            }
            FromSwarm::DialFailure(DialFailure {
                peer_id,
                connection_id,
                error,
            }) => {
                self.on_dial_failure.push(peer_id);
                self.inner
                    .on_swarm_event(FromSwarm::DialFailure(DialFailure {
                        peer_id,
                        connection_id,
                        error,
                    }));
            }
            FromSwarm::NewListener(NewListener { listener_id }) => {
                self.on_new_listener.push(listener_id);
                self.inner
                    .on_swarm_event(FromSwarm::NewListener(NewListener { listener_id }));
            }
            FromSwarm::NewListenAddr(NewListenAddr { listener_id, addr }) => {
                self.on_new_listen_addr.push((listener_id, addr.clone()));
                self.inner
                    .on_swarm_event(FromSwarm::NewListenAddr(NewListenAddr {
                        listener_id,
                        addr,
                    }));
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr { listener_id, addr }) => {
                self.on_expired_listen_addr
                    .push((listener_id, addr.clone()));
                self.inner
                    .on_swarm_event(FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
                        listener_id,
                        addr,
                    }));
            }
            FromSwarm::NewExternalAddr(NewExternalAddr { addr }) => {
                self.on_new_external_addr.push(addr.clone());
                self.inner
                    .on_swarm_event(FromSwarm::NewExternalAddr(NewExternalAddr { addr }));
            }
            FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr { addr }) => {
                self.on_expired_external_addr.push(addr.clone());
                self.inner
                    .on_swarm_event(FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr { addr }));
            }
            FromSwarm::ListenerError(ListenerError { listener_id, err }) => {
                self.on_listener_error.push(listener_id);
                self.inner
                    .on_swarm_event(FromSwarm::ListenerError(ListenerError { listener_id, err }));
            }
            FromSwarm::ListenerClosed(ListenerClosed {
                listener_id,
                reason,
            }) => {
                self.on_listener_closed.push((listener_id, reason.is_ok()));
                self.inner
                    .on_swarm_event(FromSwarm::ListenerClosed(ListenerClosed {
                        listener_id,
                        reason,
                    }));
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        p: PeerId,
        c: ConnectionId,
        e: THandlerOutEvent<Self>,
    ) {
        assert!(
            self.on_connection_established
                .iter()
                .any(|(peer_id, conn_id, ..)| *peer_id == p && c == *conn_id),
            "`on_connection_handler_event` is called for reported connections."
        );
        assert!(
            !self
                .on_connection_closed
                .iter()
                .any(|(peer_id, conn_id, ..)| *peer_id == p && c == *conn_id),
            "`on_connection_handler_event` is never called for closed connections."
        );

        self.on_connection_handler_event.push((p, c, e.clone()));
        self.inner.on_connection_handler_event(p, c, e);
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        args: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
        self.poll += 1;
        self.inner.poll(cx, args)
    }
}
