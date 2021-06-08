use crate::codec::{Message, Registration};
use crate::codec::{NewRegistration, RendezvousCodec};
use crate::protocol::Rendezvous;
use crate::{codec, protocol};
use asynchronous_codec::Framed;
use futures::{SinkExt, StreamExt};
use libp2p_core::{InboundUpgrade, OutboundUpgrade};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use log::debug;
use log::error;
use std::fmt::{Debug, Formatter};
use std::task::{Context, Poll};
use std::{fmt, mem};
use void::Void;

#[derive(Debug)]
pub struct RendezvousHandler {
    outbound: OutboundState,
    inbound: InboundState,
    keep_alive: KeepAlive,
}

impl RendezvousHandler {
    pub fn new() -> Self {
        Self {
            outbound: OutboundState::None,
            inbound: InboundState::None,
            keep_alive: KeepAlive::Yes,
        }
    }
}

pub type OutEvent = Message;

#[derive(Debug)]
pub enum InEvent {
    RegisterRequest {
        request: NewRegistration,
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
    },
    DiscoverResponse {
        discovered: Vec<Registration>,
    },
}

/// Defines what should happen next after polling an [`InboundState`] or [`OutboundState`].
enum Next<S, E> {
    /// Return from the `poll` function, either because are `Ready` or there is no more work to do (`Pending`).
    Return { poll: Poll<E>, next_state: S },
    /// Continue with polling the state.
    Continue { next_state: S },
}

/// The state of an inbound substream (i.e. the remote node opened it).
enum InboundState {
    /// There is no substream.
    None,
    /// We are in the process of reading a message from the substream.
    Reading(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We read a message, dispatched it to the behaviour and are waiting for the response.
    WaitForBehaviour(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We are in the process of sending a response.
    PendingSend(Framed<NegotiatedSubstream, RendezvousCodec>, Message),
    /// We started sending and are currently flushing the data out.
    PendingFlush(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We've sent the message and are now closing down the substream.
    Closing(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// Something went seriously wrong.
    Poisoned,
}

impl InboundState {
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, codec::Error>> {
        loop {
            let next = match mem::replace(self, InboundState::Poisoned) {
                InboundState::None => Next::Return {
                    poll: Poll::Pending,
                    next_state: InboundState::None,
                },
                InboundState::Reading(mut substream) => match substream.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok(msg))) => {
                        debug!("read message from inbound {:?}", msg);

                        if let Message::Register(..)
                        | Message::Discover { .. }
                        | Message::Unregister { .. } = msg
                        {
                            Next::Return {
                                poll: Poll::Ready(ProtocolsHandlerEvent::Custom(msg)),
                                next_state: InboundState::WaitForBehaviour(substream),
                            }
                        } else {
                            panic!("Invalid inbound message");
                        }
                    }
                    Poll::Ready(Some(Err(e))) => {
                        panic!("Error when sending outbound: {:?}", e);
                    }
                    Poll::Ready(None) => {
                        panic!("Honestly no idea what to do if this happens");
                    }
                    Poll::Pending => Next::Return {
                        poll: Poll::Pending,
                        next_state: InboundState::Reading(substream),
                    },
                },
                InboundState::WaitForBehaviour(substream) => Next::Return {
                    poll: Poll::Pending,
                    next_state: InboundState::WaitForBehaviour(substream),
                },
                InboundState::PendingSend(mut substream, message) => {
                    match substream.poll_ready_unpin(cx) {
                        Poll::Ready(Ok(())) => match substream.start_send_unpin(message) {
                            Ok(()) => Next::Continue {
                                next_state: InboundState::PendingFlush(substream),
                            },
                            Err(e) => {
                                panic!("pending send from inbound error: {:?}", e);
                            }
                        },
                        Poll::Ready(Err(e)) => {
                            panic!("pending send from inbound error: {:?}", e);
                        }
                        Poll::Pending => Next::Return {
                            poll: Poll::Pending,
                            next_state: InboundState::PendingSend(substream, message),
                        },
                    }
                }
                InboundState::PendingFlush(mut substream) => match substream.poll_flush_unpin(cx) {
                    Poll::Ready(Ok(())) => Next::Continue {
                        next_state: InboundState::Closing(substream),
                    },
                    Poll::Ready(Err(e)) => panic!("pending send from inbound error: {:?}", e),
                    Poll::Pending => Next::Return {
                        poll: Poll::Pending,
                        next_state: InboundState::PendingFlush(substream),
                    },
                },
                InboundState::Closing(mut substream) => match substream.poll_close_unpin(cx) {
                    Poll::Ready(..) => Next::Return {
                        poll: Poll::Pending,
                        next_state: InboundState::None,
                    },
                    Poll::Pending => Next::Return {
                        poll: Poll::Pending,
                        next_state: InboundState::Closing(substream),
                    },
                },
                InboundState::Poisoned => panic!("inbound poisoned"),
            };

            match next {
                Next::Return { poll, next_state } => {
                    *self = next_state;
                    return poll;
                }
                Next::Continue { next_state } => {
                    *self = next_state;
                }
            }
        }
    }
}

/// The state of an outbound substream (i.e. we opened it).
enum OutboundState {
    /// There is no substream.
    None,
    /// We got a message to send from the behaviour.
    Start(Message),
    /// We've requested a substream and are waiting for it to be set up.
    WaitingUpgrade,
    /// We got the substream, now we need to send the message.
    PendingSend(Framed<NegotiatedSubstream, RendezvousCodec>, Message),
    /// We sent the message, now we need to flush the data out.
    PendingFlush(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We are waiting for the response from the remote.
    WaitForRemote(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We got a message from the remote and dispatched it to the behaviour, now we are closing down the substream.
    Closing(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// Something went seriously wrong.
    Poisoned,
}

impl OutboundState {
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, codec::Error>> {
        loop {
            let next = match mem::replace(self, OutboundState::Poisoned) {
                OutboundState::None => Next::Return {
                    poll: Poll::Pending,
                    next_state: OutboundState::None,
                },
                OutboundState::Start(msg) => Next::Return {
                    poll: Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(Rendezvous, msg),
                    }),
                    next_state: OutboundState::WaitingUpgrade,
                },
                OutboundState::WaitingUpgrade => Next::Return {
                    poll: Poll::Pending,
                    next_state: OutboundState::WaitingUpgrade,
                },
                OutboundState::PendingSend(mut substream, message) => {
                    match substream.poll_ready_unpin(cx) {
                        Poll::Ready(Ok(())) => match substream.start_send_unpin(message) {
                            Ok(()) => Next::Continue {
                                next_state: OutboundState::PendingFlush(substream),
                            },
                            Err(e) => {
                                panic!("Error when sending outbound: {:?}", e);
                            }
                        },
                        Poll::Ready(Err(e)) => {
                            panic!("Error when sending outbound: {:?}", e);
                        }
                        Poll::Pending => Next::Return {
                            poll: Poll::Pending,
                            next_state: OutboundState::PendingSend(substream, message),
                        },
                    }
                }
                OutboundState::PendingFlush(mut substream) => {
                    match substream.poll_flush_unpin(cx) {
                        Poll::Ready(Ok(())) => Next::Continue {
                            next_state: OutboundState::WaitForRemote(substream),
                        },
                        Poll::Ready(Err(e)) => {
                            panic!("Error when flushing outbound: {:?}", e);
                        }
                        Poll::Pending => Next::Return {
                            poll: Poll::Pending,
                            next_state: OutboundState::PendingFlush(substream),
                        },
                    }
                }
                OutboundState::WaitForRemote(mut substream) => {
                    match substream.poll_next_unpin(cx) {
                        Poll::Ready(Some(Ok(msg))) => {
                            if let Message::DiscoverResponse { .. }
                            | Message::RegisterResponse { .. }
                            | Message::FailedToDiscover { .. }
                            | Message::FailedToRegister { .. } = msg
                            {
                                Next::Return {
                                    poll: Poll::Ready(ProtocolsHandlerEvent::Custom(msg)),
                                    next_state: OutboundState::Closing(substream),
                                }
                            } else {
                                panic!("Invalid inbound message");
                            }
                        }
                        Poll::Ready(Some(Err(e))) => {
                            panic!("Error when receiving message from outbound: {:?}", e)
                        }
                        Poll::Ready(None) => {
                            panic!("Honestly no idea what to do if this happens");
                        }
                        Poll::Pending => Next::Return {
                            poll: Poll::Pending,
                            next_state: OutboundState::WaitForRemote(substream),
                        },
                    }
                }
                OutboundState::Closing(mut substream) => match substream.poll_close_unpin(cx) {
                    Poll::Ready(..) => Next::Return {
                        poll: Poll::Pending,
                        next_state: OutboundState::None,
                    },
                    Poll::Pending => Next::Return {
                        poll: Poll::Pending,
                        next_state: OutboundState::Closing(substream),
                    },
                },
                OutboundState::Poisoned => {
                    panic!("outbound poisoned");
                }
            };

            match next {
                Next::Return { poll, next_state } => {
                    *self = next_state;
                    return poll;
                }
                Next::Continue { next_state } => {
                    *self = next_state;
                }
            }
        }
    }
}

impl ProtocolsHandler for RendezvousHandler {
    type InEvent = InEvent;
    type OutEvent = OutEvent;
    type Error = codec::Error;
    type InboundProtocol = protocol::Rendezvous;
    type OutboundProtocol = protocol::Rendezvous;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = Message;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        debug!("creating substream protocol");
        SubstreamProtocol::new(protocol::Rendezvous::new(), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _msg: Self::InboundOpenInfo,
    ) {
        debug!("injected inbound");
        if let InboundState::None = self.inbound {
            self.inbound = InboundState::Reading(substream);
        } else {
            unreachable!("Invalid inbound state")
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        msg: Self::OutboundOpenInfo,
    ) {
        debug!("injected outbound");
        if let OutboundState::WaitingUpgrade = self.outbound {
            self.outbound = OutboundState::PendingSend(substream, msg);
        } else {
            unreachable!("Invalid outbound state")
        }
    }

    // event injected from NotifyHandler
    fn inject_event(&mut self, req: InEvent) {
        debug!("injecting event into handler from behaviour: {:?}", &req);
        let (inbound, outbound) = match (
            req,
            mem::replace(&mut self.inbound, InboundState::Poisoned),
            mem::replace(&mut self.outbound, OutboundState::Poisoned),
        ) {
            (InEvent::RegisterRequest { request: reggo }, inbound, OutboundState::None) => {
                (inbound, OutboundState::Start(Message::Register(reggo)))
            }
            (InEvent::UnregisterRequest { namespace }, inbound, OutboundState::None) => (
                inbound,
                OutboundState::Start(Message::Unregister { namespace }),
            ),
            (InEvent::DiscoverRequest { namespace }, inbound, OutboundState::None) => (
                inbound,
                OutboundState::Start(Message::Discover { namespace }),
            ),
            (
                InEvent::RegisterResponse { ttl },
                InboundState::WaitForBehaviour(substream),
                outbound,
            ) => (
                InboundState::PendingSend(substream, Message::RegisterResponse { ttl }),
                outbound,
            ),
            (
                InEvent::DiscoverResponse { discovered },
                InboundState::WaitForBehaviour(substream),
                outbound,
            ) => (
                InboundState::PendingSend(
                    substream,
                    Message::DiscoverResponse {
                        registrations: discovered,
                    },
                ),
                outbound,
            ),
            _ => unreachable!("Handler in invalid state"),
        };

        self.inbound = inbound;
        self.outbound = outbound;
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<Void>,
    ) {
        error!("Dial upgrade error {:?}", error);
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        //todo: fix this
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
        debug!("polling handler: inbound_state: {:?}", &self.inbound);
        debug!("polling handler: outbound_state {:?}", &self.outbound);

        if let Poll::Ready(event) = self.inbound.poll(cx) {
            return Poll::Ready(event);
        }

        if let Poll::Ready(event) = self.outbound.poll(cx) {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}

impl Debug for OutboundState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            OutboundState::None => f.write_str("none"),
            OutboundState::Start(_) => f.write_str("start"),
            OutboundState::WaitingUpgrade => f.write_str("waiting_upgrade"),
            OutboundState::PendingSend(_, _) => f.write_str("pending_send"),
            OutboundState::PendingFlush(_) => f.write_str("pending_flush"),
            OutboundState::WaitForRemote(_) => f.write_str("waiting_for_remote"),
            OutboundState::Closing(_) => f.write_str("closing"),
            OutboundState::Poisoned => f.write_str("poisoned"),
        }
    }
}

impl Debug for InboundState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            InboundState::None => f.write_str("none"),
            InboundState::Reading(_) => f.write_str("reading"),
            InboundState::PendingSend(_, _) => f.write_str("pending_send"),
            InboundState::PendingFlush(_) => f.write_str("pending_flush"),
            InboundState::WaitForBehaviour(_) => f.write_str("waiting_for_behaviour"),
            InboundState::Closing(_) => f.write_str("closing"),
            InboundState::Poisoned => f.write_str("poisoned"),
        }
    }
}
