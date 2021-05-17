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

use crate::v2::client::transport;
use crate::v2::protocol::{inbound_stop, outbound_hop};
use futures::channel::mpsc::Sender;
use futures::channel::oneshot;
use futures::future::FutureExt;
use futures::stream::FuturesUnordered;
use futures_timer::Delay;
use libp2p_core::either::EitherError;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::{upgrade, ConnectedPoint, Multiaddr, PeerId};
use libp2p_swarm::protocols_handler::{InboundUpgradeSend, OutboundUpgradeSend};
use libp2p_swarm::{
    IntoProtocolsHandler, KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use std::collections::VecDeque;
use std::task::{Context, Poll};
use std::time::Instant;

pub enum In {
    Reserve {
        to_listener: Sender<transport::ToListenerMsg>,
    },
    EstablishCircuit {
        dst_peer_id: PeerId,
        send_back:
            oneshot::Sender<Result<super::RelayedConnection, transport::OutgoingRelayReqError>>,
    },
}

pub enum Event {
    Reserved,
}

pub struct Prototype {
    local_peer_id: PeerId,
}

impl Prototype {
    pub(crate) fn new(local_peer_id: PeerId) -> Self {
        Self { local_peer_id }
    }
}

impl IntoProtocolsHandler for Prototype {
    type Handler = Handler;

    fn into_handler(self, remote_peer_id: &PeerId, _endpoint: &ConnectedPoint) -> Self::Handler {
        Handler {
            remote_peer_id: *remote_peer_id,
            local_peer_id: self.local_peer_id,
            queued_events: Default::default(),
            reservation: None,
            alive_lend_out_substreams: Default::default(),
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        inbound_stop::Upgrade {}
    }
}

pub struct Handler {
    local_peer_id: PeerId,
    remote_peer_id: PeerId,

    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ProtocolsHandlerEvent<
            <Self as ProtocolsHandler>::OutboundProtocol,
            <Self as ProtocolsHandler>::OutboundOpenInfo,
            <Self as ProtocolsHandler>::OutEvent,
            <Self as ProtocolsHandler>::Error,
        >,
    >,

    reservation: Option<Reservation>,

    /// Tracks substreams lend out to the transport.
    ///
    /// Contains a [`futures::future::Future`] for each lend out substream that
    /// resolves once the substream is dropped.
    ///
    /// Once all substreams are dropped and this handler has no other work,
    /// [`KeepAlive::Until`] can be set, allowing the connection to be closed
    /// eventually.
    alive_lend_out_substreams: FuturesUnordered<oneshot::Receiver<()>>,
}

struct Reservation {
    renewal_timeout: Delay,
    pending_msgs: VecDeque<transport::ToListenerMsg>,
    to_listener: Sender<transport::ToListenerMsg>,
}

impl ProtocolsHandler for Handler {
    type InEvent = In;
    type OutEvent = Event;
    type Error = ProtocolsHandlerUpgrErr<
        EitherError<inbound_stop::UpgradeError, outbound_hop::UpgradeError>,
    >;
    type InboundProtocol = inbound_stop::Upgrade;
    type OutboundProtocol = outbound_hop::Upgrade;
    type OutboundOpenInfo = OutboundOpenInfo;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(inbound_stop::Upgrade {}, ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        inbound_circuit: <Self::InboundProtocol as upgrade::InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
        if let Some(reservation) = &mut self.reservation {
            let src_peer_id = inbound_circuit.src_peer_id();
            let (tx, rx) = oneshot::channel();
            self.alive_lend_out_substreams.push(rx);
            let connection = super::RelayedConnection::new_inbound(inbound_circuit, tx);
            reservation.pending_msgs.push_back(
                transport::ToListenerMsg::IncomingRelayedConnection {
                    stream: connection,
                    src_peer_id,
                    relay_peer_id: self.remote_peer_id,
                    // TODO: Fix
                    relay_addr: Multiaddr::empty(),
                },
            )
        } else {
            todo!("deny")
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        output: <Self::OutboundProtocol as upgrade::OutboundUpgrade<NegotiatedSubstream>>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        match (output, info) {
            (
                outbound_hop::Output::Reservation { expire, addrs },
                OutboundOpenInfo::Reserve { to_listener },
            ) => {
                self.queued_events
                    .push_back(ProtocolsHandlerEvent::Custom(Event::Reserved));
                match self.reservation {
                    Some(_) => todo!(),
                    None => {
                        let local_peer_id = self.local_peer_id;
                        let mut pending_msgs = VecDeque::new();
                        pending_msgs.push_back(transport::ToListenerMsg::Reservation {
                            addrs: addrs
                                .into_iter()
                                .map(|a| a.with(Protocol::P2p(local_peer_id.into())))
                                .collect(),
                        });
                        self.reservation = Some(Reservation {
                            // TODO: Should timeout fire early to make sure reservation never expires?
                            renewal_timeout: Delay::new(
                                expire
                                    .expect("TODO handle where no expire")
                                    .checked_duration_since(Instant::now())
                                    .unwrap(),
                            ),
                            pending_msgs,
                            to_listener,
                        })
                    }
                }
            }
            (
                outbound_hop::Output::Circuit {
                    substream,
                    read_buffer,
                },
                OutboundOpenInfo::Connect { send_back },
            ) => {
                let (tx, rx) = oneshot::channel();
                self.alive_lend_out_substreams.push(rx);
                let _ = send_back.send(Ok(super::RelayedConnection::new_outbound(
                    substream,
                    read_buffer,
                    tx,
                )));
            }
            _ => unreachable!(),
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            In::Reserve { to_listener } => {
                self.queued_events
                    .push_back(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            outbound_hop::Upgrade::Reserve,
                            OutboundOpenInfo::Reserve { to_listener },
                        ),
                    });
            }
            In::EstablishCircuit {
                send_back,
                dst_peer_id,
            } => {
                self.queued_events
                    .push_back(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            outbound_hop::Upgrade::Connect { dst_peer_id },
                            OutboundOpenInfo::Connect { send_back },
                        ),
                    });
            }
        }
    }

    fn inject_listen_upgrade_error(
        &mut self,
        _: Self::InboundOpenInfo,
        _error: ProtocolsHandlerUpgrErr<<Self::InboundProtocol as InboundUpgradeSend>::Error>,
    ) {
        todo!()
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _open_info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        panic!("{:?}", error)
    }

    // TODO: Why is this not a mut reference? If it were the case, we could do all keep alive handling in here.
    fn connection_keep_alive(&self) -> KeepAlive {
        // TODO
        // TODO: cover lend out substreams.
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

        if let Some(reservation) = &mut self.reservation {
            if !reservation.pending_msgs.is_empty() {
                match reservation.to_listener.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        reservation
                            .to_listener
                            .start_send(
                                reservation
                                    .pending_msgs
                                    .pop_front()
                                    .expect("Called !is_empty()."),
                            )
                            .unwrap();
                    }
                    Poll::Ready(Err(e)) => todo!("{:?}", e),
                    Poll::Pending => {}
                }
            }
            match reservation.renewal_timeout.poll_unpin(cx) {
                Poll::Ready(()) => todo!(),
                Poll::Pending => {}
            }
        }

        Poll::Pending
    }
}

pub enum OutboundOpenInfo {
    Reserve {
        to_listener: Sender<transport::ToListenerMsg>,
    },
    Connect {
        send_back:
            oneshot::Sender<Result<super::RelayedConnection, transport::OutgoingRelayReqError>>,
    },
}
