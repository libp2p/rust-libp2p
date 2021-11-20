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
    DialError, IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
    ProtocolsHandler,
};
use libp2p_core::{
    connection::{ConnectionId, ListenerId},
    multiaddr::Multiaddr,
    ConnectedPoint, PeerId,
};
use std::collections::HashMap;
use std::task::{Context, Poll};

/// A `MockBehaviour` is a `NetworkBehaviour` that allows for
/// the instrumentation of return values, without keeping
/// any further state.
pub struct MockBehaviour<THandler, TOutEvent>
where
    THandler: ProtocolsHandler,
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
    THandler: ProtocolsHandler,
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
    THandler: ProtocolsHandler + Clone,
    THandler::OutEvent: Clone,
    TOutEvent: Send + 'static,
{
    type ProtocolsHandler = THandler;
    type OutEvent = TOutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
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
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
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
    pub inject_connected: Vec<PeerId>,
    pub inject_disconnected: Vec<PeerId>,
    pub inject_connection_established: Vec<(PeerId, ConnectionId, ConnectedPoint)>,
    pub inject_connection_closed: Vec<(PeerId, ConnectionId, ConnectedPoint)>,
    pub inject_event: Vec<(
        PeerId,
        ConnectionId,
        <<TInner::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
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
            inject_connected: Vec::new(),
            inject_disconnected: Vec::new(),
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
        self.inject_connected = Vec::new();
        self.inject_disconnected = Vec::new();
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
            assert_eq!(self.inject_disconnected.len(), expected_disconnections);
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
            assert_eq!(self.inject_connected.len(), expected_connections);
            return true;
        }

        false
    }
}

impl<TInner> NetworkBehaviour for CallTraceBehaviour<TInner>
where
    TInner: NetworkBehaviour,
    <<TInner::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent:
        Clone,
{
    type ProtocolsHandler = TInner::ProtocolsHandler;
    type OutEvent = TInner::OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        self.inner.new_handler()
    }

    fn addresses_of_peer(&mut self, p: &PeerId) -> Vec<Multiaddr> {
        self.addresses_of_peer.push(p.clone());
        self.inner.addresses_of_peer(p)
    }

    fn inject_connected(&mut self, peer: &PeerId) {
        assert!(
            self.inject_connection_established
                .iter()
                .any(|(peer_id, _, _)| peer_id == peer),
            "`inject_connected` is called after at least one `inject_connection_established`."
        );
        self.inject_connected.push(peer.clone());
        self.inner.inject_connected(peer);
    }

    fn inject_connection_established(
        &mut self,
        p: &PeerId,
        c: &ConnectionId,
        e: &ConnectedPoint,
        errors: Option<&Vec<Multiaddr>>,
    ) {
        self.inject_connection_established
            .push((p.clone(), c.clone(), e.clone()));
        self.inner.inject_connection_established(p, c, e, errors);
    }

    fn inject_disconnected(&mut self, peer: &PeerId) {
        assert!(
            self.inject_connection_closed
                .iter()
                .any(|(peer_id, _, _)| peer_id == peer),
            "`inject_disconnected` is called after at least one `inject_connection_closed`."
        );
        self.inject_disconnected.push(*peer);
        self.inner.inject_disconnected(peer);
    }

    fn inject_connection_closed(
        &mut self,
        p: &PeerId,
        c: &ConnectionId,
        e: &ConnectedPoint,
        handler: <Self::ProtocolsHandler as IntoProtocolsHandler>::Handler,
    ) {
        let connection = (p.clone(), c.clone(), e.clone());
        assert!(
            self.inject_connection_established.contains(&connection),
            "`inject_connection_closed` is called only for connections for \
            which `inject_connection_established` was called first."
        );
        self.inject_connection_closed.push(connection);
        self.inner.inject_connection_closed(p, c, e, handler);
    }

    fn inject_event(
        &mut self,
        p: PeerId,
        c: ConnectionId,
        e: <<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
    ) {
        assert!(
            self.inject_connection_established
                .iter()
                .any(|(peer_id, conn_id, _)| *peer_id == p && c == *conn_id),
            "`inject_event` is called for reported connections."
        );
        assert!(
            !self
                .inject_connection_closed
                .iter()
                .any(|(peer_id, conn_id, _)| *peer_id == p && c == *conn_id),
            "`inject_event` is never called for closed connections."
        );

        self.inject_event.push((p.clone(), c.clone(), e.clone()));
        self.inner.inject_event(p, c, e);
    }

    fn inject_dial_failure(
        &mut self,
        p: Option<PeerId>,
        handler: Self::ProtocolsHandler,
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
        self.inject_listener_error.push(l.clone());
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
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        self.poll += 1;
        self.inner.poll(cx, args)
    }
}
