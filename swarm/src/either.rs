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

use crate::protocols_handler::{either::IntoEitherHandler, IntoProtocolsHandler, ProtocolsHandler};
use crate::{
    DialError, NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess,
    PollParameters,
};
use either::Either;
use libp2p_core::{
    connection::{ConnectionId, ListenerId},
    ConnectedPoint, Multiaddr, PeerId,
};
use std::{task::Context, task::Poll};

/// Implementation of [`NetworkBehaviour`] that can be either of two implementations.
impl<L, R> NetworkBehaviour for Either<L, R>
where
    L: NetworkBehaviour,
    R: NetworkBehaviour,
{
    type ProtocolsHandler = IntoEitherHandler<L::ProtocolsHandler, R::ProtocolsHandler>;
    type OutEvent = Either<L::OutEvent, R::OutEvent>;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        match self {
            Either::Left(a) => IntoEitherHandler::Left(a.new_handler()),
            Either::Right(b) => IntoEitherHandler::Right(b.new_handler()),
        }
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        match self {
            Either::Left(a) => a.addresses_of_peer(peer_id),
            Either::Right(b) => b.addresses_of_peer(peer_id),
        }
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        match self {
            Either::Left(a) => a.inject_connected(peer_id),
            Either::Right(b) => b.inject_connected(peer_id),
        };
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        match self {
            Either::Left(a) => a.inject_disconnected(peer_id),
            Either::Right(b) => b.inject_disconnected(peer_id),
        }
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        endpoint: &ConnectedPoint,
        errors: Option<&Vec<Multiaddr>>,
    ) {
        match self {
            Either::Left(a) => {
                a.inject_connection_established(peer_id, connection, endpoint, errors)
            }
            Either::Right(b) => {
                b.inject_connection_established(peer_id, connection, endpoint, errors)
            }
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        endpoint: &ConnectedPoint,
        handler: <Self::ProtocolsHandler as IntoProtocolsHandler>::Handler,
    ) {
        match (self, handler) {
            (Either::Left(behaviour), Either::Left(handler)) => {
                behaviour.inject_connection_closed(peer_id, connection, endpoint, handler)
            }
            (Either::Right(behaviour), Either::Right(handler)) => {
                behaviour.inject_connection_closed(peer_id, connection, endpoint, handler)
            }
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
        event: <<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
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

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ProtocolsHandler,
        error: &DialError,
    ) {
        match (self, handler) {
            (Either::Left(behaviour), IntoEitherHandler::Left(handler)) => {
                behaviour.inject_dial_failure(peer_id, handler, error)
            }
            (Either::Right(behaviour), IntoEitherHandler::Right(handler)) => {
                behaviour.inject_dial_failure(peer_id, handler, error)
            }
            _ => unreachable!(),
        }
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &Multiaddr,
        send_back_addr: &Multiaddr,
        handler: Self::ProtocolsHandler,
    ) {
        match (self, handler) {
            (Either::Left(behaviour), IntoEitherHandler::Left(handler)) => {
                behaviour.inject_listen_failure(local_addr, send_back_addr, handler)
            }
            (Either::Right(behaviour), IntoEitherHandler::Right(handler)) => {
                behaviour.inject_listen_failure(local_addr, send_back_addr, handler)
            }
            _ => unreachable!(),
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
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        let event = match self {
            Either::Left(behaviour) => futures::ready!(behaviour.poll(cx, params))
                .map_out(|e| Either::Left(e))
                .map_handler_and_in(|h| IntoEitherHandler::Left(h), |e| Either::Left(e)),
            Either::Right(behaviour) => futures::ready!(behaviour.poll(cx, params))
                .map_out(|e| Either::Right(e))
                .map_handler_and_in(|h| IntoEitherHandler::Right(h), |e| Either::Right(e)),
        };

        Poll::Ready(event)
    }
}

impl<TEvent, TBehaviourLeft, TBehaviourRight> NetworkBehaviourEventProcess<TEvent>
    for Either<TBehaviourLeft, TBehaviourRight>
where
    TBehaviourLeft: NetworkBehaviourEventProcess<TEvent>,
    TBehaviourRight: NetworkBehaviourEventProcess<TEvent>,
{
    fn inject_event(&mut self, event: TEvent) {
        match self {
            Either::Left(a) => a.inject_event(event),
            Either::Right(b) => b.inject_event(event),
        }
    }
}
