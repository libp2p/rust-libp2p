use crate::codec::{
    self, Cookie, ErrorCode, Message, NewRegistration, Registration, RendezvousCodec,
};
use crate::protocol;
use crate::substream::{Advance, Next, SubstreamState};
use asynchronous_codec::Framed;
use futures::{SinkExt, StreamExt};
use libp2p_core::{InboundUpgrade, OutboundUpgrade};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use std::fmt::Debug;
use std::mem;
use std::task::{Context, Poll};
use void::Void;

pub struct RendezvousHandler {
    outbound: SubstreamState<Outbound>,
    inbound: SubstreamState<Inbound>,
}

impl RendezvousHandler {
    pub fn new() -> Self {
        Self {
            outbound: SubstreamState::None,
            inbound: SubstreamState::None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum OutEvent {
    RegistrationRequested(NewRegistration),
    Registered {
        namespace: String,
        ttl: i64,
    },
    RegisterFailed {
        namespace: String,
        error: ErrorCode,
    },
    UnregisterRequested {
        namespace: String,
    },
    DiscoverRequested {
        namespace: Option<String>,
        cookie: Option<Cookie>,
    },
    Discovered {
        registrations: Vec<Registration>,
        cookie: Cookie,
    },
    DiscoverFailed {
        namespace: Option<String>,
        error: ErrorCode,
    },
}

#[derive(Debug)]
pub enum InEvent {
    RegisterRequest {
        request: NewRegistration,
    },
    DeclineRegisterRequest {
        error: ErrorCode,
    },
    UnregisterRequest {
        namespace: String,
    },
    DiscoverRequest {
        namespace: Option<String>,
        cookie: Option<Cookie>,
    },
    RegisterResponse {
        ttl: i64,
    },
    DiscoverResponse {
        discovered: Vec<Registration>,
        cookie: Cookie,
    },
}

/// The state of an inbound substream (i.e. the remote node opened it).
enum Inbound {
    /// We are in the process of reading a message from the substream.
    Reading(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We read a message, dispatched it to the behaviour and are waiting for the response.
    PendingBehaviour(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We are in the process of sending a response.
    PendingSend(Framed<NegotiatedSubstream, RendezvousCodec>, Message),
    /// We started sending and are currently flushing the data out.
    PendingFlush(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We've sent the message and are now closing down the substream.
    PendingClose(Framed<NegotiatedSubstream, RendezvousCodec>),
}

/// The state of an outbound substream (i.e. we opened it).
enum Outbound {
    /// We got a message to send from the behaviour.
    Start(Message),
    /// We've requested a substream and are waiting for it to be set up.
    PendingSubstream,
    /// We got the substream, now we need to send the message.
    PendingSend {
        substream: Framed<NegotiatedSubstream, RendezvousCodec>,
        to_send: Message,
    },
    /// We sent the message, now we need to flush the data out.
    PendingFlush {
        substream: Framed<NegotiatedSubstream, RendezvousCodec>,
        sent_message: Message,
    },
    /// We are waiting for the response from the remote.
    PendingRemote {
        substream: Framed<NegotiatedSubstream, RendezvousCodec>,
        sent_message: Message,
    },
    /// We are closing down the substream.
    PendingClose(Framed<NegotiatedSubstream, RendezvousCodec>),
}

/// Errors that can occur while interacting with a substream.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Reading message {0:?} at this stage is a protocol violation")]
    BadMessage(Message),
    #[error("Failed to write message to substream")]
    WriteMessage(#[source] codec::Error),
    #[error("Failed to read message from substream")]
    ReadMessage(#[source] codec::Error),
    #[error("Substream ended unexpectedly mid-protocol")]
    UnexpectedEndOfStream,
}

impl Advance for Inbound {
    type Event = ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, Error>;

    fn advance(self, cx: &mut Context<'_>) -> Next<Self, Self::Event> {
        match self {
            Inbound::Reading(mut substream) => match substream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    let event = match msg {
                        Message::Register(new_registration) => {
                            OutEvent::RegistrationRequested(new_registration)
                        }
                        Message::Discover { cookie, namespace } => {
                            OutEvent::DiscoverRequested { cookie, namespace }
                        }
                        Message::Unregister { namespace } => {
                            OutEvent::UnregisterRequested { namespace }
                        }
                        other => {
                            return Next::Done {
                                event: Some(ProtocolsHandlerEvent::Close(Error::BadMessage(other))),
                            }
                        }
                    };

                    Next::Return {
                        poll: Poll::Ready(ProtocolsHandlerEvent::Custom(event)),
                        next_state: Inbound::PendingBehaviour(substream),
                    }
                }
                Poll::Ready(Some(Err(e))) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::ReadMessage(e))),
                },
                Poll::Ready(None) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::UnexpectedEndOfStream)),
                },
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Inbound::Reading(substream),
                },
            },
            Inbound::PendingBehaviour(substream) => Next::Return {
                poll: Poll::Pending,
                next_state: Inbound::PendingBehaviour(substream),
            },
            Inbound::PendingSend(mut substream, message) => match substream.poll_ready_unpin(cx) {
                Poll::Ready(Ok(())) => match substream.start_send_unpin(message) {
                    Ok(()) => Next::Continue {
                        next_state: Inbound::PendingFlush(substream),
                    },
                    Err(e) => Next::Done {
                        event: Some(ProtocolsHandlerEvent::Close(Error::WriteMessage(e))),
                    },
                },
                Poll::Ready(Err(e)) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::WriteMessage(e))),
                },
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Inbound::PendingSend(substream, message),
                },
            },
            Inbound::PendingFlush(mut substream) => match substream.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Continue {
                    next_state: Inbound::PendingClose(substream),
                },
                Poll::Ready(Err(e)) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::WriteMessage(e))),
                },
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Inbound::PendingFlush(substream),
                },
            },
            Inbound::PendingClose(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Done { event: None },
                Poll::Ready(Err(_)) => Next::Done { event: None }, // there is nothing we can do about an error during close
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Inbound::PendingClose(substream),
                },
            },
        }
    }
}

impl Advance for Outbound {
    type Event = ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, Error>;

    fn advance(self, cx: &mut Context<'_>) -> Next<Self, Self::Event> {
        match self {
            Outbound::Start(msg) => Next::Return {
                poll: Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(protocol::new(), msg),
                }),
                next_state: Outbound::PendingSubstream,
            },
            Outbound::PendingSubstream => Next::Return {
                poll: Poll::Pending,
                next_state: Outbound::PendingSubstream,
            },
            Outbound::PendingSend {
                mut substream,
                to_send: message,
            } => match substream.poll_ready_unpin(cx) {
                Poll::Ready(Ok(())) => match substream.start_send_unpin(message.clone()) {
                    Ok(()) => Next::Continue {
                        next_state: Outbound::PendingFlush {
                            substream,
                            sent_message: message,
                        },
                    },
                    Err(e) => Next::Done {
                        event: Some(ProtocolsHandlerEvent::Close(Error::WriteMessage(e))),
                    },
                },
                Poll::Ready(Err(e)) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::WriteMessage(e))),
                },
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Outbound::PendingSend {
                        substream,
                        to_send: message,
                    },
                },
            },
            Outbound::PendingFlush {
                mut substream,
                sent_message,
            } => match substream.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Continue {
                    next_state: Outbound::PendingRemote {
                        substream,
                        sent_message,
                    },
                },
                Poll::Ready(Err(e)) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::WriteMessage(e))),
                },
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Outbound::PendingFlush {
                        substream,
                        sent_message,
                    },
                },
            },
            Outbound::PendingRemote {
                mut substream,
                sent_message,
            } => match substream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(received_message))) => {
                    use Message::*;
                    use OutEvent::*;

                    let event = match (sent_message, received_message) {
                        (Register(registration), RegisterResponse(Ok(ttl))) => Registered {
                            namespace: registration.namespace,
                            ttl,
                        },
                        (Register(registration), RegisterResponse(Err(error))) => RegisterFailed {
                            namespace: registration.namespace,
                            error,
                        },
                        (Discover { .. }, DiscoverResponse(Ok((registrations, cookie)))) => {
                            Discovered {
                                registrations,
                                cookie,
                            }
                        }
                        (Discover { namespace, .. }, DiscoverResponse(Err(error))) => {
                            DiscoverFailed { namespace, error }
                        }
                        (_, other) => {
                            return Next::Done {
                                event: Some(ProtocolsHandlerEvent::Close(Error::BadMessage(other))),
                            }
                        }
                    };

                    Next::Return {
                        poll: Poll::Ready(ProtocolsHandlerEvent::Custom(event)),
                        next_state: Outbound::PendingClose(substream),
                    }
                }
                Poll::Ready(Some(Err(e))) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::ReadMessage(e))),
                },
                Poll::Ready(None) => Next::Done {
                    event: Some(ProtocolsHandlerEvent::Close(Error::UnexpectedEndOfStream)),
                },
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Outbound::PendingRemote {
                        substream,
                        sent_message,
                    },
                },
            },
            Outbound::PendingClose(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Done { event: None },
                Poll::Ready(Err(_)) => Next::Done { event: None }, // there is nothing we can do about an error during close
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Outbound::PendingClose(substream),
                },
            },
        }
    }
}

impl ProtocolsHandler for RendezvousHandler {
    type InEvent = InEvent;
    type OutEvent = OutEvent;
    type Error = Error;
    type InboundProtocol = protocol::Rendezvous;
    type OutboundProtocol = protocol::Rendezvous;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = Message;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::new(), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _msg: Self::InboundOpenInfo,
    ) {
        if let SubstreamState::None = self.inbound {
            self.inbound = SubstreamState::Active(Inbound::Reading(substream));
        } else {
            unreachable!("Invalid inbound state")
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        msg: Self::OutboundOpenInfo,
    ) {
        if let SubstreamState::Active(Outbound::PendingSubstream) = self.outbound {
            self.outbound = SubstreamState::Active(Outbound::PendingSend {
                substream: substream,
                to_send: msg,
            });
        } else {
            unreachable!("Invalid outbound state")
        }
    }

    // event injected from NotifyHandler
    fn inject_event(&mut self, req: InEvent) {
        let (inbound, outbound) = match (
            req,
            mem::replace(&mut self.inbound, SubstreamState::Poisoned),
            mem::replace(&mut self.outbound, SubstreamState::Poisoned),
        ) {
            (InEvent::RegisterRequest { request: reggo }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::Start(Message::Register(reggo))),
            ),
            (InEvent::UnregisterRequest { namespace }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::Start(Message::Unregister { namespace })),
            ),
            (InEvent::DiscoverRequest { namespace, cookie }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::Start(Message::Discover { namespace, cookie })),
            ),
            (
                InEvent::RegisterResponse { ttl },
                SubstreamState::Active(Inbound::PendingBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::RegisterResponse(Ok(ttl)),
                )),
                outbound,
            ),
            (
                InEvent::DeclineRegisterRequest { error },
                SubstreamState::Active(Inbound::PendingBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::RegisterResponse(Err(error)),
                )),
                outbound,
            ),
            (
                InEvent::DiscoverResponse { discovered, cookie },
                SubstreamState::Active(Inbound::PendingBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::DiscoverResponse(Ok((discovered, cookie))),
                )),
                outbound,
            ),
            _ => unreachable!("Handler in invalid state"),
        };

        self.inbound = inbound;
        self.outbound = outbound;
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ProtocolsHandlerUpgrErr<Void>,
    ) {
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
        if let Poll::Ready(event) = self.inbound.poll(cx) {
            return Poll::Ready(event);
        }

        if let Poll::Ready(event) = self.outbound.poll(cx) {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
