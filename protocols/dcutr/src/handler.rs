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

use crate::protocol;
use either::Either;
use libp2p_core::connection::ConnectionId;
use libp2p_core::upgrade::{self, DeniedUpgrade};
use libp2p_core::{ConnectedPoint, PeerId};
use libp2p_swarm::dummy;
use libp2p_swarm::handler::SendWrapper;
use libp2p_swarm::{ConnectionHandler, IntoConnectionHandler};

pub mod direct;
pub mod relayed;

pub enum Prototype {
    DirectConnection {
        role: Role,
        relayed_connection_id: ConnectionId,
    },
    UnknownConnection,
}

pub enum Role {
    Initiator { attempt: u8 },
    Listener,
}

impl IntoConnectionHandler for Prototype {
    type Handler = Either<relayed::Handler, Either<direct::Handler, dummy::ConnectionHandler>>;

    fn into_handler(self, _remote_peer_id: &PeerId, endpoint: &ConnectedPoint) -> Self::Handler {
        match self {
            Self::UnknownConnection => {
                if endpoint.is_relayed() {
                    Either::Left(relayed::Handler::new(endpoint.clone()))
                } else {
                    Either::Right(Either::Right(dummy::ConnectionHandler))
                }
            }
            Self::DirectConnection {
                relayed_connection_id,
                ..
            } => {
                assert!(
                    !endpoint.is_relayed(),
                    "`Prototype::DirectConnection` is never created for relayed connection."
                );
                Either::Right(Either::Left(direct::Handler::new(relayed_connection_id)))
            }
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
        match self {
            Prototype::UnknownConnection => upgrade::EitherUpgrade::A(SendWrapper(
                upgrade::EitherUpgrade::A(protocol::inbound::Upgrade {}),
            )),
            Prototype::DirectConnection { .. } => {
                upgrade::EitherUpgrade::A(SendWrapper(upgrade::EitherUpgrade::B(DeniedUpgrade)))
            }
        }
    }
}
