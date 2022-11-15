// Copyright 2021 Protocol Labs.
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

use crate::behaviour::THandlerInEvent;
use crate::handler::ConnectionHandler;
use crate::{DialError, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use either::Either;
use libp2p_core::{
    connection::ConnectionId, transport::ListenerId, ConnectedPoint, Multiaddr, PeerId,
};
use std::{task::Context, task::Poll};

/// Implementation of [`NetworkBehaviour`] that can be either of two implementations.
impl<L, R> NetworkBehaviour for Either<L, R>
where
    L: NetworkBehaviour,
    R: NetworkBehaviour,
{
    type ConnectionHandler = Either<L::ConnectionHandler, R::ConnectionHandler>;
    type OutEvent = Either<L::OutEvent, R::OutEvent>;

    fn new_handler(
        &mut self,
        peer: &PeerId,
        connected_point: &ConnectedPoint,
    ) -> Self::ConnectionHandler {
        match self {
            Either::Left(a) => Either::Left(a.new_handler(peer, connected_point)),
            Either::Right(b) => Either::Right(b.new_handler(peer, connected_point)),
        }
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        match self {
            Either::Left(a) => a.addresses_of_peer(peer_id),
            Either::Right(b) => b.addresses_of_peer(peer_id),
        }
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        endpoint: &ConnectedPoint,
        errors: Option<&Vec<Multiaddr>>,
        other_established: usize,
    ) {
        match self {
            Either::Left(a) => a.inject_connection_established(
                peer_id,
                connection,
                endpoint,
                errors,
                other_established,
            ),
            Either::Right(b) => b.inject_connection_established(
                peer_id,
                connection,
                endpoint,
                errors,
                other_established,
            ),
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        endpoint: &ConnectedPoint,
        handler: Self::ConnectionHandler,
        remaining_established: usize,
    ) {
        match (self, handler) {
            (Either::Left(behaviour), Either::Left(handler)) => behaviour.inject_connection_closed(
                peer_id,
                connection,
                endpoint,
                handler,
                remaining_established,
            ),
            (Either::Right(behaviour), Either::Right(handler)) => behaviour
                .inject_connection_closed(
                    peer_id,
                    connection,
                    endpoint,
                    handler,
                    remaining_established,
                ),
            _ => unreachable!(),
        }
    }

    fn inject_address_change(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        match self {
            Either::Left(a) => a.inject_address_change(peer_id, connection, old, new),
            Either::Right(b) => b.inject_address_change(peer_id, connection, old, new),
        }
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection: ConnectionId,
        event: <Self::ConnectionHandler as ConnectionHandler>::OutEvent,
    ) {
        match (self, event) {
            (Either::Left(behaviour), Either::Left(event)) => {
                behaviour.inject_event(peer_id, connection, event)
            }
            (Either::Right(behaviour), Either::Right(event)) => {
                behaviour.inject_event(peer_id, connection, event)
            }
            _ => unreachable!(),
        }
    }

    fn inject_dial_failure(&mut self, peer_id: Option<PeerId>, error: &DialError) {
        match self {
            Either::Left(behaviour) => behaviour.inject_dial_failure(peer_id, error),
            Either::Right(behaviour) => behaviour.inject_dial_failure(peer_id, error),
        }
    }

    fn inject_listen_failure(&mut self, local_addr: &Multiaddr, send_back_addr: &Multiaddr) {
        match self {
            Either::Left(behaviour) => behaviour.inject_listen_failure(local_addr, send_back_addr),
            Either::Right(behaviour) => behaviour.inject_listen_failure(local_addr, send_back_addr),
        }
    }

    fn inject_new_listener(&mut self, id: ListenerId) {
        match self {
            Either::Left(a) => a.inject_new_listener(id),
            Either::Right(b) => b.inject_new_listener(id),
        }
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        match self {
            Either::Left(a) => a.inject_new_listen_addr(id, addr),
            Either::Right(b) => b.inject_new_listen_addr(id, addr),
        }
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        match self {
            Either::Left(a) => a.inject_expired_listen_addr(id, addr),
            Either::Right(b) => b.inject_expired_listen_addr(id, addr),
        }
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        match self {
            Either::Left(a) => a.inject_new_external_addr(addr),
            Either::Right(b) => b.inject_new_external_addr(addr),
        }
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        match self {
            Either::Left(a) => a.inject_expired_external_addr(addr),
            Either::Right(b) => b.inject_expired_external_addr(addr),
        }
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        match self {
            Either::Left(a) => a.inject_listener_error(id, err),
            Either::Right(b) => b.inject_listener_error(id, err),
        }
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        match self {
            Either::Left(a) => a.inject_listener_closed(id, reason),
            Either::Right(b) => b.inject_listener_closed(id, reason),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            Self::OutEvent,
            Either<THandlerInEvent<L::ConnectionHandler>, THandlerInEvent<R::ConnectionHandler>>,
        >,
    > {
        let event = match self {
            Either::Left(behaviour) => futures::ready!(behaviour.poll(cx, params))
                .map_out(Either::Left)
                .map_in(Either::Left),
            Either::Right(behaviour) => futures::ready!(behaviour.poll(cx, params))
                .map_out(Either::Right)
                .map_in(Either::Right),
        };

        Poll::Ready(event)
    }
}
