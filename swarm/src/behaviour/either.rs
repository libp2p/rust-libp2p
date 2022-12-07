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

use crate::behaviour::{
    self, inject_from_swarm, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
};
use crate::behaviour::{ConnectionDenied, THandlerInEvent};
use either::Either;
use libp2p_core::{ConnectedPoint, Multiaddr, PeerId};
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
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        Ok(match self {
            Either::Left(a) => Either::Left(a.new_handler(peer, connected_point)?),
            Either::Right(b) => Either::Right(b.new_handler(peer, connected_point)?),
        })
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        match self {
            Either::Left(a) => a.addresses_of_peer(peer_id),
            Either::Right(b) => b.addresses_of_peer(peer_id),
        }
    }

    fn on_swarm_event(&mut self, event: behaviour::FromSwarm<Self::ConnectionHandler>) {
        match self {
            Either::Left(b) => inject_from_swarm(
                b,
                event.map_handler(|h| match h {
                    Either::Left(h) => h,
                    Either::Right(_) => unreachable!(),
                }),
            ),
            Either::Right(b) => inject_from_swarm(
                b,
                event.map_handler(|h| match h {
                    Either::Right(h) => h,
                    Either::Left(_) => unreachable!(),
                }),
            ),
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: libp2p_core::connection::ConnectionId,
        event: crate::THandlerOutEvent<Self>,
    ) {
        match (self, event) {
            (Either::Left(left), Either::Left(event)) => {
                #[allow(deprecated)]
                left.inject_event(peer_id, connection_id, event);
            }
            (Either::Right(right), Either::Right(event)) => {
                #[allow(deprecated)]
                right.inject_event(peer_id, connection_id, event);
            }
            _ => unreachable!(),
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
