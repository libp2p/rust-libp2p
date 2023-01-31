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

use crate::behaviour::{self, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use crate::connection::ConnectionId;
use crate::handler::either::IntoEitherHandler;
use crate::THandlerOutEvent;
use either::Either;
use libp2p_core::{Multiaddr, PeerId};
use std::{task::Context, task::Poll};

/// Implementation of [`NetworkBehaviour`] that can be either of two implementations.
impl<L, R> NetworkBehaviour for Either<L, R>
where
    L: NetworkBehaviour,
    R: NetworkBehaviour,
{
    type ConnectionHandler = IntoEitherHandler<L::ConnectionHandler, R::ConnectionHandler>;
    type OutEvent = Either<L::OutEvent, R::OutEvent>;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
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

    fn on_swarm_event(&mut self, event: behaviour::FromSwarm<Self::ConnectionHandler>) {
        match self {
            Either::Left(b) => b.on_swarm_event(event.map_handler(
                |h| h.unwrap_left(),
                |h| match h {
                    Either::Left(h) => h,
                    Either::Right(_) => unreachable!(),
                },
            )),
            Either::Right(b) => b.on_swarm_event(event.map_handler(
                |h| h.unwrap_right(),
                |h| match h {
                    Either::Right(h) => h,
                    Either::Left(_) => unreachable!(),
                },
            )),
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match (self, event) {
            (Either::Left(left), Either::Left(event)) => {
                left.on_connection_handler_event(peer_id, connection_id, event);
            }
            (Either::Right(right), Either::Right(event)) => {
                right.on_connection_handler_event(peer_id, connection_id, event);
            }
            _ => unreachable!(),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        let event = match self {
            Either::Left(behaviour) => futures::ready!(behaviour.poll(cx, params))
                .map_out(Either::Left)
                .map_handler_and_in(IntoEitherHandler::Left, Either::Left),
            Either::Right(behaviour) => futures::ready!(behaviour.poll(cx, params))
                .map_out(Either::Right)
                .map_handler_and_in(IntoEitherHandler::Right, Either::Right),
        };

        Poll::Ready(event)
    }
}
