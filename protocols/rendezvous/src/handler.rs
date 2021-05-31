use crate::codec::{Message, Registration};
use crate::codec::{NewRegistration, RendezvousCodec};
use crate::protocol;
use crate::protocol::Rendezvous;
use asynchronous_codec::Framed;
use futures::{SinkExt, StreamExt};
use libp2p_core::{AuthenticatedPeerRecord, InboundUpgrade, OutboundUpgrade, PeerId};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use log::error;
use std::task::{Context, Poll};
use void::Void;

pub struct RendezvousHandler {
    outbound_substream: OutboundState,
    inbound_substream: InboundState,
    keep_alive: KeepAlive,
}

impl RendezvousHandler {
    pub fn new() -> Self {
        Self {
            outbound_substream: OutboundState::None,
            inbound_substream: InboundState::None,
            keep_alive: KeepAlive::Yes,
        }
    }
}

pub struct HandlerEvent(pub Message);

#[derive(Debug)]
pub enum Input {
    RegisterRequest {
        namespace: String,
        ttl: Option<i64>,
        // TODO: Signed peer record field
    },
    UnregisterRequest {
        namespace: String,
        // TODO: what is the `id` field here in the PB message
    },
    DiscoverRequest {
        namespace: Option<String>,
        // TODO limit: Option<i64>
        // TODO cookie: Option<Vec<u8>
    },
    RegisterResponse {
        ttl: i64,
        message: Message,
    },
    DiscoverResponse {
        record: AuthenticatedPeerRecord,
        discovered: Vec<(String, PeerId)>,
    },
}

/// State of the inbound substream, opened either by us or by the remote.
enum InboundState {
    None,
    /// Waiting for behaviour to respond to the inbound substream
    Reading(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// Waiting for behaviour to respond to the inbound substream
    WaitForBehaviour(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// Waiting to send response to remote
    PendingSend(Framed<NegotiatedSubstream, RendezvousCodec>, Message),
    PendingFlush(Framed<NegotiatedSubstream, RendezvousCodec>),
    Closing(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// An error occurred during processing.
    Poisoned,
}

/// State of the outbound substream, opened either by us or by the remote.
enum OutboundState {
    None,
    Start(Message),
    WaitingUpgrade,
    /// Waiting to send a message to the remote.
    PendingSend(Framed<NegotiatedSubstream, RendezvousCodec>, Message),
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// Waiting for remote to respond on the outbound substream
    WaitForRemote(Framed<NegotiatedSubstream, RendezvousCodec>),
    Closing(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// An error occurred during processing.
    Poisoned,
}

impl ProtocolsHandler for RendezvousHandler {
    type InEvent = Input;
    type OutEvent = HandlerEvent;
    type Error = crate::codec::Error;
    type InboundOpenInfo = ();
    type InboundProtocol = protocol::Rendezvous;
    type OutboundOpenInfo = Message;
    type OutboundProtocol = protocol::Rendezvous;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        let rendezvous_protocol = crate::protocol::Rendezvous::new();
        SubstreamProtocol::new(rendezvous_protocol, ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _msg: Self::InboundOpenInfo,
    ) {
        if let InboundState::None = self.inbound_substream {
            self.inbound_substream = InboundState::Reading(substream);
        } else {
            unreachable!("Invalid inbound state")
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        msg: Self::OutboundOpenInfo,
    ) {
        if let OutboundState::WaitingUpgrade = self.outbound_substream {
            self.outbound_substream = OutboundState::PendingSend(substream, msg);
        } else {
            unreachable!("Invalid outbound state")
        }
    }

    // event injected from NotifyHandler
    fn inject_event(&mut self, req: Input) {
        let (inbound_substream, outbound_substream) = match (
            req,
            std::mem::replace(&mut self.inbound_substream, InboundState::Poisoned),
            std::mem::replace(&mut self.outbound_substream, OutboundState::Poisoned),
        ) {
            (Input::RegisterRequest { namespace, ttl }, inbound, OutboundState::None) => (
                inbound,
                OutboundState::Start(Message::Register(NewRegistration::new(
                    namespace.clone(),
                    todo!(),
                    ttl,
                ))),
            ),
            (Input::UnregisterRequest { namespace }, inbound, OutboundState::None) => (
                inbound,
                OutboundState::Start(Message::Unregister {
                    namespace: namespace.clone(),
                }),
            ),
            (Input::DiscoverRequest { namespace }, inbound, OutboundState::None) => (
                inbound,
                OutboundState::Start(Message::Discover {
                    namespace: namespace.clone(),
                }),
            ),
            (
                Input::RegisterResponse { ttl: _ttl, message },
                InboundState::WaitForBehaviour(substream),
                outbound,
            ) => (InboundState::PendingSend(substream, message), outbound),
            (
                Input::DiscoverResponse { record, discovered },
                InboundState::WaitForBehaviour(substream),
                outbound,
            ) => {
                let msg = Message::DiscoverResponse {
                    registrations: discovered
                        .iter()
                        .map(|d| Registration {
                            namespace: d.0.to_string(),
                            record: record.clone(),
                        })
                        .collect(),
                };
                (InboundState::PendingSend(substream, msg), outbound)
            }
            _ => unreachable!("Handler in invalid state"),
        };

        self.inbound_substream = inbound_substream;
        self.outbound_substream = outbound_substream;
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<Void>,
    ) {
        error!("Dial upgrade error {:?}", error);
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
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
        match std::mem::replace(&mut self.inbound_substream, InboundState::Poisoned) {
            InboundState::PendingSend(mut substream, message) => {
                match substream.poll_ready_unpin(cx) {
                    Poll::Ready(Ok(())) => match substream.start_send_unpin(message) {
                        Ok(()) => {
                            self.inbound_substream = InboundState::PendingFlush(substream);
                        }
                        Err(e) => {
                            return Poll::Ready(ProtocolsHandlerEvent::Close(e));
                        }
                    },
                    Poll::Ready(Err(e)) => {
                        return Poll::Ready(ProtocolsHandlerEvent::Close(e));
                    }
                    Poll::Pending => {
                        self.keep_alive = KeepAlive::Yes;
                        self.inbound_substream = InboundState::PendingSend(substream, message);
                    }
                }
            }
            InboundState::PendingFlush(mut substream) => match substream.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => {
                    self.inbound_substream = InboundState::None;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(ProtocolsHandlerEvent::Close(e)),
                Poll::Pending => {
                    self.keep_alive = KeepAlive::Yes;
                    self.inbound_substream = InboundState::PendingFlush(substream);
                }
            },
            InboundState::Reading(mut substream) => match substream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    self.inbound_substream = InboundState::WaitForBehaviour(substream);
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(HandlerEvent(msg)));
                }
                Poll::Ready(Some(Err(e))) => {
                    error!("Error when sending outbound: {:?}", e);
                }
                Poll::Ready(None) => {
                    error!("Honestly no idea what to do if this happens");
                }
                Poll::Pending => {
                    self.inbound_substream = InboundState::Reading(substream);
                }
            },
            InboundState::WaitForBehaviour(substream) => {
                self.inbound_substream = InboundState::WaitForBehaviour(substream);
            }
            InboundState::Closing(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(..) => {
                    if let OutboundState::None | OutboundState::Poisoned = self.outbound_substream {
                        self.keep_alive = KeepAlive::No;
                    }
                    self.inbound_substream = InboundState::None;
                }
                Poll::Pending => {
                    self.inbound_substream = InboundState::Closing(substream);
                }
            },
            InboundState::None => self.outbound_substream = OutboundState::None,
            InboundState::Poisoned => self.outbound_substream = OutboundState::None,
        }

        match std::mem::replace(&mut self.outbound_substream, OutboundState::Poisoned) {
            OutboundState::Start(msg) => {
                self.outbound_substream = OutboundState::WaitingUpgrade;
                return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(Rendezvous, msg),
                });
            }
            OutboundState::WaitingUpgrade => {
                self.outbound_substream = OutboundState::WaitingUpgrade;
            }
            OutboundState::PendingSend(mut substream, message) => {
                match substream.poll_ready_unpin(cx) {
                    Poll::Ready(Ok(())) => match substream.start_send_unpin(message) {
                        Ok(()) => {
                            self.outbound_substream = OutboundState::PendingFlush(substream);
                        }
                        Err(e) => {
                            error!("Error when sending outbound: {:?}", e);
                            return Poll::Ready(ProtocolsHandlerEvent::Close(e));
                        }
                    },
                    Poll::Ready(Err(e)) => {
                        return Poll::Ready(ProtocolsHandlerEvent::Close(e));
                    }
                    Poll::Pending => {
                        self.keep_alive = KeepAlive::Yes;
                        self.outbound_substream = OutboundState::PendingSend(substream, message);
                    }
                }
            }
            OutboundState::PendingFlush(mut substream) => match substream.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => {
                    self.outbound_substream = OutboundState::WaitForRemote(substream)
                }
                Poll::Ready(Err(e)) => {
                    error!("Error when flushing outbound: {:?}", e);
                    return Poll::Ready(ProtocolsHandlerEvent::Close(e));
                }
                Poll::Pending => {
                    self.keep_alive = KeepAlive::Yes;
                    self.outbound_substream = OutboundState::PendingFlush(substream);
                }
            },
            OutboundState::WaitForRemote(mut substream) => match substream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    self.outbound_substream = OutboundState::Closing(substream);
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(HandlerEvent(msg)));
                }
                Poll::Ready(Some(Err(e))) => {
                    self.outbound_substream = OutboundState::Closing(substream);
                    error!("Error when receiving message from outbound: {:?}", e)
                }
                Poll::Ready(None) => {
                    error!("Honestly no idea what to do if this happens");
                }
                Poll::Pending => {
                    self.outbound_substream = OutboundState::WaitForRemote(substream);
                }
            },
            OutboundState::Closing(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(..) => {
                    if let InboundState::None | InboundState::Poisoned = self.inbound_substream {
                        self.keep_alive = KeepAlive::No;
                    }
                    self.outbound_substream = OutboundState::None;
                }
                Poll::Pending => {
                    self.outbound_substream = OutboundState::Closing(substream);
                }
            },
            OutboundState::None => self.outbound_substream = OutboundState::None,
            OutboundState::Poisoned => {
                self.outbound_substream = {
                    error!("outbound poisoned");
                    OutboundState::Poisoned
                }
            }
        }

        Poll::Pending
    }
}
