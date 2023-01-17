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
use crate::{THandler, THandlerInEvent, THandlerOutEvent};
use either::Either;
use libp2p_core::connection::ConnectionId;
use libp2p_core::{Endpoint, Multiaddr, PeerId};
use std::error::Error;
use std::{task::Context, task::Poll};

/// Implementation of [`NetworkBehaviour`] that can be either of two implementations.
impl<L, R> NetworkBehaviour for Either<L, R>
where
    L: NetworkBehaviour,
    R: NetworkBehaviour,
{
    type ConnectionHandler = Either<THandler<L>, THandler<R>>;
    type OutEvent = Either<L::OutEvent, R::OutEvent>;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), Box<dyn Error + Send + 'static>> {
        match self {
            Either::Left(inner) => {
                inner.handle_pending_inbound_connection(connection_id, local_addr, remote_addr)?
            }
            Either::Right(inner) => {
                inner.handle_pending_inbound_connection(connection_id, local_addr, remote_addr)?
            }
        };

        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        peer: PeerId,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, Box<dyn Error + Send + 'static>> {
        match self {
            Either::Left(inner) => Ok(Either::Left(inner.handle_established_inbound_connection(
                peer,
                connection_id,
                local_addr,
                remote_addr,
            )?)),
            Either::Right(inner) => {
                Ok(Either::Right(inner.handle_established_inbound_connection(
                    peer,
                    connection_id,
                    local_addr,
                    remote_addr,
                )?))
            }
        }
    }

    fn handle_pending_outbound_connection(
        &mut self,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
        connection_id: ConnectionId,
    ) -> Result<Vec<Multiaddr>, Box<dyn Error + Send + 'static>> {
        let addresses = match self {
            Either::Left(inner) => inner.handle_pending_outbound_connection(
                maybe_peer,
                addresses,
                effective_role,
                connection_id,
            )?,
            Either::Right(inner) => inner.handle_pending_outbound_connection(
                maybe_peer,
                addresses,
                effective_role,
                connection_id,
            )?,
        };

        Ok(addresses)
    }

    fn handle_established_outbound_connection(
        &mut self,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        connection_id: ConnectionId,
    ) -> Result<THandler<Self>, Box<dyn Error + Send + 'static>> {
        match self {
            Either::Left(inner) => Ok(Either::Left(inner.handle_established_outbound_connection(
                peer,
                addr,
                role_override,
                connection_id,
            )?)),
            Either::Right(inner) => Ok(Either::Right(
                inner.handle_established_outbound_connection(
                    peer,
                    addr,
                    role_override,
                    connection_id,
                )?,
            )),
        }
    }

    fn on_swarm_event(&mut self, event: behaviour::FromSwarm<Self::ConnectionHandler>) {
        match self {
            Either::Left(b) => b.on_swarm_event(event.map_handler(|h| match h {
                Either::Left(h) => h,
                Either::Right(_) => unreachable!(),
            })),
            Either::Right(b) => b.on_swarm_event(event.map_handler(|h| match h {
                Either::Right(h) => h,
                Either::Left(_) => unreachable!(),
            })),
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: libp2p_core::connection::ConnectionId,
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
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
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
