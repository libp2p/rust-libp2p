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

use crate::{
    ConnectionHandler, DialError, IntoConnectionHandler, NetworkBehaviour, NetworkBehaviourAction,
    PollParameters,
};
use libp2p_core::{
    connection::ConnectionId, multiaddr::Multiaddr, transport::ListenerId, ConnectedPoint, PeerId,
};
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
    pub next_action: Option<NetworkBehaviourAction<TOutEvent, THandler>>,
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

    fn inject_event(&mut self, _: PeerId, _: ConnectionId, _: THandler::OutEvent) {}

    fn poll(
        &mut self,
        _: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        self.next_action.take().map_or(Poll::Pending, Poll::Ready)
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
    pub inject_connection_established: Vec<(PeerId, ConnectionId, ConnectedPoint, usize)>,
    pub inject_connection_closed: Vec<(PeerId, ConnectionId, ConnectedPoint, usize)>,
    pub inject_event: Vec<(
        PeerId,
        ConnectionId,
        <<TInner::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
    )>,
    pub inject_dial_failure: Vec<Option<PeerId>>,
    pub inject_new_listener: Vec<ListenerId>,
    pub inject_new_listen_addr: Vec<(ListenerId, Multiaddr)>,
    pub inject_new_external_addr: Vec<Multiaddr>,
    pub inject_expired_listen_addr: Vec<(ListenerId, Multiaddr)>,
    pub inject_expired_external_addr: Vec<Multiaddr>,
    pub inject_listener_error: Vec<ListenerId>,
    pub inject_listener_closed: Vec<(ListenerId, bool)>,
    pub poll: usize,
}

impl<TInner> CallTraceBehaviour<TInner>
where
    TInner: NetworkBehaviour,
{
    pub fn new(inner: TInner) -> Self {
        Self {
            inner,
            addresses_of_peer: Vec::new(),
            inject_connection_established: Vec::new(),
            inject_connection_closed: Vec::new(),
            inject_event: Vec::new(),
            inject_dial_failure: Vec::new(),
            inject_new_listener: Vec::new(),
            inject_new_listen_addr: Vec::new(),
            inject_new_external_addr: Vec::new(),
            inject_expired_listen_addr: Vec::new(),
            inject_expired_external_addr: Vec::new(),
            inject_listener_error: Vec::new(),
            inject_listener_closed: Vec::new(),
            poll: 0,
        }
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.addresses_of_peer = Vec::new();
        self.inject_connection_established = Vec::new();
        self.inject_connection_closed = Vec::new();
        self.inject_event = Vec::new();
        self.inject_dial_failure = Vec::new();
        self.inject_new_listen_addr = Vec::new();
        self.inject_new_external_addr = Vec::new();
        self.inject_expired_listen_addr = Vec::new();
        self.inject_listener_error = Vec::new();
        self.inject_listener_closed = Vec::new();
        self.poll = 0;
    }

    pub fn inner(&mut self) -> &mut TInner {
        &mut self.inner
    }

    pub fn num_connections_to_peer(&self, peer: PeerId) -> usize {
        self.inject_connection_established
            .iter()
            .filter(|(peer_id, _, _, _)| *peer_id == peer)
            .count()
            - self
                .inject_connection_closed
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
        if self.inject_connection_closed.len() == expected_closed_connections {
            assert_eq!(
                self.inject_connection_closed
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
        if self.inject_connection_established.len() == expected_established_connections {
            assert_eq!(
                self.inject_connection_established
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

    fn inject_connection_established(
        &mut self,
        p: &PeerId,
        c: &ConnectionId,
        e: &ConnectedPoint,
        errors: Option<&Vec<Multiaddr>>,
        other_established: usize,
    ) {
        let mut other_peer_connections = self
            .inject_connection_established
            .iter()
            .rev() // take last to first
            .filter_map(|(peer, .., other_established)| {
                if p == peer {
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
        self.inject_connection_established
            .push((*p, *c, e.clone(), other_established));
        self.inner
            .inject_connection_established(p, c, e, errors, other_established);
    }

    fn inject_connection_closed(
        &mut self,
        p: &PeerId,
        c: &ConnectionId,
        e: &ConnectedPoint,
        handler: <Self::ConnectionHandler as IntoConnectionHandler>::Handler,
        remaining_established: usize,
    ) {
        let mut other_closed_connections = self
            .inject_connection_established
            .iter()
            .rev() // take last to first
            .filter_map(|(peer, .., remaining_established)| {
                if p == peer {
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
            self.inject_connection_established
                .iter()
                .any(|(peer, conn_id, endpoint, _)| (peer, conn_id, endpoint) == (p, c, e)),
            "`inject_connection_closed` is called only for connections for \
            which `inject_connection_established` was called first."
        );
        self.inject_connection_closed
            .push((*p, *c, e.clone(), remaining_established));
        self.inner
            .inject_connection_closed(p, c, e, handler, remaining_established);
    }

    fn inject_event(
        &mut self,
        p: PeerId,
        c: ConnectionId,
        e: <<Self::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
    ) {
        assert!(
            self.inject_connection_established
                .iter()
                .any(|(peer_id, conn_id, ..)| *peer_id == p && c == *conn_id),
            "`inject_event` is called for reported connections."
        );
        assert!(
            !self
                .inject_connection_closed
                .iter()
                .any(|(peer_id, conn_id, ..)| *peer_id == p && c == *conn_id),
            "`inject_event` is never called for closed connections."
        );

        self.inject_event.push((p, c, e.clone()));
        self.inner.inject_event(p, c, e);
    }

    fn inject_dial_failure(
        &mut self,
        p: Option<PeerId>,
        handler: Self::ConnectionHandler,
        error: &DialError,
    ) {
        self.inject_dial_failure.push(p);
        self.inner.inject_dial_failure(p, handler, error);
    }

    fn inject_new_listener(&mut self, id: ListenerId) {
        self.inject_new_listener.push(id);
        self.inner.inject_new_listener(id);
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, a: &Multiaddr) {
        self.inject_new_listen_addr.push((id, a.clone()));
        self.inner.inject_new_listen_addr(id, a);
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, a: &Multiaddr) {
        self.inject_expired_listen_addr.push((id, a.clone()));
        self.inner.inject_expired_listen_addr(id, a);
    }

    fn inject_new_external_addr(&mut self, a: &Multiaddr) {
        self.inject_new_external_addr.push(a.clone());
        self.inner.inject_new_external_addr(a);
    }

    fn inject_expired_external_addr(&mut self, a: &Multiaddr) {
        self.inject_expired_external_addr.push(a.clone());
        self.inner.inject_expired_external_addr(a);
    }

    fn inject_listener_error(&mut self, l: ListenerId, e: &(dyn std::error::Error + 'static)) {
        self.inject_listener_error.push(l);
        self.inner.inject_listener_error(l, e);
    }

    fn inject_listener_closed(&mut self, l: ListenerId, r: Result<(), &std::io::Error>) {
        self.inject_listener_closed.push((l, r.is_ok()));
        self.inner.inject_listener_closed(l, r);
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        args: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        self.poll += 1;
        self.inner.poll(cx, args)
    }
}
