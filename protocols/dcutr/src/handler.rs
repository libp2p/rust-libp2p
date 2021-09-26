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
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p_core::connection::ConnectionId;
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::{upgrade, ConnectedPoint, PeerId};
use libp2p_swarm::protocols_handler::DummyProtocolsHandler;
use libp2p_swarm::protocols_handler::{InboundUpgradeSend, OutboundUpgradeSend, SendWrapper};
use libp2p_swarm::{
    IntoProtocolsHandler, KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use std::collections::VecDeque;
use std::fmt;
use std::task::{Context, Poll};

pub enum In {
    Connect {
        obs_addrs: Vec<Multiaddr>,
        // Use new-type.
        attempt: u8,
    },
    AcceptInboundConnect {
        obs_addrs: Vec<Multiaddr>,
        inbound_connect: protocol::InboundConnect,
    },
}

impl fmt::Debug for In {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            In::Connect { obs_addrs, attempt } => f
                .debug_struct("In::Connect")
                .field("obs_addrs", obs_addrs)
                .field("attempt", attempt)
                .finish(),
            In::AcceptInboundConnect {
                obs_addrs,
                inbound_connect: _,
            } => f
                .debug_struct("In::AcceptInboundConnect")
                .field("obs_addrs", obs_addrs)
                .finish(),
        }
    }
}

pub enum Event {
    InboundConnectReq {
        inbound_connect: protocol::InboundConnect,
        remote_addr: Multiaddr,
    },
    // TODO: Rename to InboundConnectNegotiated?
    InboundConnectNeg(Vec<Multiaddr>),
    OutboundConnectNeg {
        remote_addrs: Vec<Multiaddr>,
        attempt: u8,
    },
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::InboundConnectReq {
                inbound_connect: _,
                remote_addr,
            } => f
                .debug_struct("Event::InboundConnectReq")
                .field("remote_addrs", remote_addr)
                .finish(),
            Event::InboundConnectNeg(addrs) => f
                .debug_tuple("Event::InboundConnectNeg")
                .field(addrs)
                .finish(),
            Event::OutboundConnectNeg {
                remote_addrs,
                attempt,
            } => f
                .debug_struct("Event::OutboundConnectNeg")
                .field("remote_addrs", remote_addrs)
                .field("attempt", attempt)
                .finish(),
        }
    }
}

pub enum Prototype {
    RelayedConnection,
    DirectConnection { role: Role },
    UnknownConnection,
}

pub enum Role {
    Initiator {
        attempt: u8,
        relay_connection_id: ConnectionId,
    },
    Listener,
}

impl IntoProtocolsHandler for Prototype {
    type Handler = Either<Handler, DummyProtocolsHandler>;

    fn into_handler(self, _remote_peer_id: &PeerId, endpoint: &ConnectedPoint) -> Self::Handler {
        let is_relayed_addr = match endpoint {
            ConnectedPoint::Dialer { address } => address,
            ConnectedPoint::Listener { local_addr, .. } => local_addr,
        }
        .iter()
        .any(|p| p == Protocol::P2pCircuit);

        match (self, is_relayed_addr) {
            (Self::RelayedConnection | Self::UnknownConnection, true) => {
                // TODO: When handler is created via new_handler, the connection is inbound. It should only
                // ever be us initiating a dcutr request on this handler then, as one never initiates a
                // request on an outbound handler. Should this be enforced?
                Either::Left(Handler {
                    remote_addr: endpoint.get_remote_address().clone(),
                    queued_events: Default::default(),
                    inbound_connects: Default::default(),
                })
            }
            (Self::DirectConnection { .. } | Self::UnknownConnection, false) => {
                Either::Right(DummyProtocolsHandler::default())
            }
            (Self::RelayedConnection, false) => {
                todo!("Expected relayed connection.")
            }
            (Self::DirectConnection { .. }, true) => {
                todo!("Expected non-relayed connection.")
            }
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        upgrade::EitherUpgrade::A(SendWrapper(protocol::InboundUpgrade {}))
    }
}

pub struct Handler {
    remote_addr: Multiaddr,
    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ProtocolsHandlerEvent<
            <Self as ProtocolsHandler>::OutboundProtocol,
            <Self as ProtocolsHandler>::OutboundOpenInfo,
            <Self as ProtocolsHandler>::OutEvent,
            <Self as ProtocolsHandler>::Error,
        >,
    >,

    inbound_connects:
        FuturesUnordered<BoxFuture<'static, Result<Vec<Multiaddr>, protocol::InboundUpgradeError>>>,
}

impl ProtocolsHandler for Handler {
    type InEvent = In;
    type OutEvent = Event;
    type Error = ProtocolsHandlerUpgrErr<std::io::Error>;
    type InboundProtocol = protocol::InboundUpgrade;
    type OutboundProtocol = protocol::OutboundUpgrade;
    // TODO: Use new type for attempt instead of u8.
    type OutboundOpenInfo = u8;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::InboundUpgrade {}, ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        inbound_connect: <Self::InboundProtocol as upgrade::InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
        self.queued_events
            .push_back(ProtocolsHandlerEvent::Custom(Event::InboundConnectReq {
                inbound_connect,
                remote_addr: self.remote_addr.clone(),
            }));
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol::Connect { obs_addrs }: <Self::OutboundProtocol as upgrade::OutboundUpgrade<
            NegotiatedSubstream,
        >>::Output,
        attempt: Self::OutboundOpenInfo,
    ) {
        self.queued_events
            .push_back(ProtocolsHandlerEvent::Custom(Event::OutboundConnectNeg {
                remote_addrs: obs_addrs,
                attempt,
            }));
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            In::Connect { obs_addrs, attempt } => {
                self.queued_events
                    .push_back(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            protocol::OutboundUpgrade::new(obs_addrs),
                            attempt,
                        ),
                    });
            }
            In::AcceptInboundConnect {
                inbound_connect,
                obs_addrs,
            } => {
                self.inbound_connects
                    .push(inbound_connect.accept(obs_addrs).boxed());
            }
        }
    }

    fn inject_listen_upgrade_error(
        &mut self,
        _: Self::InboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<<Self::InboundProtocol as InboundUpgradeSend>::Error>,
    ) {
        todo!("{:?}", error)
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _open_info: Self::OutboundOpenInfo,
        _error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        todo!()
    }

    // TODO: Why is this not a mut reference? If it were the case, we could do all keep alive handling in here.
    fn connection_keep_alive(&self) -> KeepAlive {
        // TODO
        //
        // How about a KeepAlive::Until of ~10 seconds, to enable coordination of hole punching
        // retries on same relayed connection.
        KeepAlive::Yes
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        // Return queued events.
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        while let Poll::Ready(Some(remote_addrs)) = self.inbound_connects.poll_next_unpin(cx) {
            return Poll::Ready(ProtocolsHandlerEvent::Custom(Event::InboundConnectNeg(
                remote_addrs.unwrap(),
            )));
        }

        Poll::Pending
    }
}
