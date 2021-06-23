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
use std::task::{Context, Poll};
use std::{fmt, mem};
use void::Void;

pub struct RendezvousHandler {
    outbound: SubstreamState<Outbound>,
    outbound_history: MessageHistory,
    inbound: SubstreamState<Inbound>,
}

impl Default for RendezvousHandler {
    fn default() -> Self {
        Self {
            outbound: SubstreamState::None,
            outbound_history: Default::default(),
            inbound: SubstreamState::None,
        }
    }
}

#[derive(Default)]
struct MessageHistory {
    sent: Vec<Message>,
}

impl MessageHistory {
    fn clear(&mut self) {
        self.sent.clear();
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
        limit: Option<i64>,
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
    RegisterRequest(NewRegistration),
    DeclineRegisterRequest(ErrorCode),
    UnregisterRequest {
        namespace: String,
    },
    DiscoverRequest {
        namespace: Option<String>,
        cookie: Option<Cookie>,
        limit: Option<i64>,
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
    PendingRead(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We read a message, dispatched it to the behaviour and are waiting for the response.
    PendingBehaviour(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We are in the process of sending a response.
    PendingSend(Framed<NegotiatedSubstream, RendezvousCodec>, Message),
    /// We've sent the message and are now closing down the substream.
    PendingClose(Framed<NegotiatedSubstream, RendezvousCodec>),
}

impl fmt::Debug for Inbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Inbound::PendingRead(_) => write!(f, "Inbound::PendingRead"),
            Inbound::PendingBehaviour(_) => write!(f, "Inbound::PendingBehaviour"),
            Inbound::PendingSend(_, _) => write!(f, "Inbound::PendingSend"),
            Inbound::PendingClose(_) => write!(f, "Inbound::PendingClose"),
        }
    }
}

/// The state of an outbound substream (i.e. we opened it).
enum Outbound {
    /// We got a message to send from the behaviour.
    PendingOpen(Message),
    /// We've requested a substream and are waiting for it to be negotiated.
    PendingNegotiate,
    /// We got the substream, now we need to send the message.
    PendingSend {
        substream: Framed<NegotiatedSubstream, RendezvousCodec>,
        to_send: Message,
    },
    /// We sent the message, now we need to flush the data out.
    PendingFlush(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We are waiting for the response from the remote.
    PendingRemote(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We are closing down the substream.
    PendingClose(Framed<NegotiatedSubstream, RendezvousCodec>),
}

impl fmt::Debug for Outbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outbound::PendingOpen(_) => write!(f, "Outbound::PendingOpen"),
            Outbound::PendingNegotiate => write!(f, "Outbound::PendingNegotiate"),
            Outbound::PendingSend { .. } => write!(f, "Outbound::PendingSend"),
            Outbound::PendingFlush(_) => write!(f, "Outbound::PendingFlush"),
            Outbound::PendingRemote(_) => write!(f, "Outbound::PendingRemote"),
            Outbound::PendingClose(_) => write!(f, "Outbound::PendingClose"),
        }
    }
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

impl<'handler> Advance<'handler> for Inbound {
    type Event = OutEvent;
    type Params = ();
    type Error = Error;
    type Protocol = SubstreamProtocol<protocol::Rendezvous, Message>;

    fn advance(
        self,
        cx: &mut Context<'_>,
        _: &mut Self::Params,
    ) -> Result<Next<Self, Self::Event, Self::Protocol>, Self::Error> {
        Ok(match self {
            Inbound::PendingRead(mut substream) => {
                match substream.poll_next_unpin(cx).map_err(Error::ReadMessage)? {
                    Poll::Ready(Some(msg)) => {
                        let event = match msg.clone() {
                            Message::Register(registration) => {
                                OutEvent::RegistrationRequested(registration)
                            }
                            Message::Discover {
                                cookie,
                                namespace,
                                limit,
                            } => OutEvent::DiscoverRequested {
                                cookie,
                                namespace,
                                limit,
                            },
                            Message::Unregister { namespace } => {
                                OutEvent::UnregisterRequested { namespace }
                            }
                            other => return Err(Error::BadMessage(other)),
                        };

                        Next::EmitEvent {
                            event,
                            next_state: Inbound::PendingBehaviour(substream),
                        }
                    }
                    Poll::Ready(None) => return Err(Error::UnexpectedEndOfStream),
                    Poll::Pending => Next::Pending {
                        next_state: Inbound::PendingRead(substream),
                    },
                }
            }
            Inbound::PendingBehaviour(substream) => Next::Pending {
                next_state: Inbound::PendingBehaviour(substream),
            },
            Inbound::PendingSend(mut substream, message) => match substream
                .poll_ready_unpin(cx)
                .map_err(Error::WriteMessage)?
            {
                Poll::Ready(()) => {
                    substream
                        .start_send_unpin(message.clone())
                        .map_err(Error::WriteMessage)?;

                    Next::Continue {
                        next_state: Inbound::PendingClose(substream),
                    }
                }
                Poll::Pending => Next::Pending {
                    next_state: Inbound::PendingSend(substream, message),
                },
            },
            Inbound::PendingClose(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Done,
                Poll::Ready(Err(_)) => Next::Done, // there is nothing we can do about an error during close
                Poll::Pending => Next::Pending {
                    next_state: Inbound::PendingClose(substream),
                },
            },
        })
    }
}

struct OutboundPollParams<'handler> {
    history: &'handler mut MessageHistory,
}

impl<'handler> Advance<'handler> for Outbound {
    type Event = OutEvent;
    type Params = OutboundPollParams<'handler>;
    type Error = Error;
    type Protocol = SubstreamProtocol<protocol::Rendezvous, Message>;

    fn advance(
        self,
        cx: &mut Context<'_>,
        OutboundPollParams { history }: &mut OutboundPollParams,
    ) -> Result<Next<Self, Self::Event, Self::Protocol>, Self::Error> {
        Ok(match self {
            Outbound::PendingOpen(msg) => Next::OpenSubstream {
                protocol: SubstreamProtocol::new(protocol::new(), msg),
                next_state: Outbound::PendingNegotiate,
            },
            Outbound::PendingNegotiate => Next::Pending {
                next_state: Outbound::PendingNegotiate,
            },
            Outbound::PendingSend {
                mut substream,
                to_send: message,
            } => match substream
                .poll_ready_unpin(cx)
                .map_err(Error::WriteMessage)?
            {
                Poll::Ready(()) => {
                    substream
                        .start_send_unpin(message.clone())
                        .map_err(Error::WriteMessage)?;
                    history.sent.push(message);

                    Next::Continue {
                        next_state: Outbound::PendingFlush(substream),
                    }
                }
                Poll::Pending => Next::Pending {
                    next_state: Outbound::PendingSend {
                        substream,
                        to_send: message,
                    },
                },
            },
            Outbound::PendingFlush(mut substream) => match substream
                .poll_flush_unpin(cx)
                .map_err(Error::WriteMessage)?
            {
                Poll::Ready(()) => Next::Continue {
                    next_state: Outbound::PendingRemote(substream),
                },
                Poll::Pending => Next::Pending {
                    next_state: Outbound::PendingFlush(substream),
                },
            },
            Outbound::PendingRemote(mut substream) => match substream
                .poll_next_unpin(cx)
                .map_err(Error::ReadMessage)?
            {
                Poll::Ready(Some(received_message)) => {
                    use Message::*;
                    use OutEvent::*;

                    // Absolutely amazing Rust pattern matching ahead!
                    // We match against the slice of historical messages and the received message.
                    // [<message>, ..] effectively matches against the first message that we sent on this substream
                    let event = match (history.sent.as_slice(), received_message) {
                        ([Register(registration), ..], RegisterResponse(Ok(ttl))) => Registered {
                            namespace: registration.namespace.to_owned(),
                            ttl,
                        },
                        ([Register(registration), ..], RegisterResponse(Err(error))) => {
                            RegisterFailed {
                                namespace: registration.namespace.to_owned(),
                                error,
                            }
                        }
                        ([Discover { .. }, ..], DiscoverResponse(Ok((registrations, cookie)))) => {
                            Discovered {
                                registrations,
                                cookie,
                            }
                        }
                        ([Discover { namespace, .. }, ..], DiscoverResponse(Err(error))) => {
                            DiscoverFailed {
                                namespace: namespace.to_owned(),
                                error,
                            }
                        }
                        (.., other) => return Err(Error::BadMessage(other)),
                    };

                    Next::EmitEvent {
                        event,
                        next_state: Outbound::PendingClose(substream),
                    }
                }
                Poll::Ready(None) => return Err(Error::UnexpectedEndOfStream),
                Poll::Pending => Next::Pending {
                    next_state: Outbound::PendingRemote(substream),
                },
            },
            Outbound::PendingClose(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Done,
                Poll::Ready(Err(_)) => Next::Done, // there is nothing we can do about an error during close
                Poll::Pending => Next::Pending {
                    next_state: Outbound::PendingClose(substream),
                },
            },
        })
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
        match self.inbound {
            SubstreamState::None => {
                self.inbound = SubstreamState::Active(Inbound::PendingRead(substream));
            }
            _ => {
                log::warn!("Ignoring new inbound substream because existing one is still active")
            }
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        msg: Self::OutboundOpenInfo,
    ) {
        match self.outbound {
            SubstreamState::Active(Outbound::PendingNegotiate) => {
                self.outbound = SubstreamState::Active(Outbound::PendingSend {
                    substream,
                    to_send: msg,
                });
                self.outbound_history.clear();
            }
            _ => {
                log::warn!("Ignoring new outbound substream because existing one is still active")
            }
        }
    }

    // event injected from NotifyHandler
    fn inject_event(&mut self, req: InEvent) {
        let (inbound, outbound) = match (
            req,
            mem::replace(&mut self.inbound, SubstreamState::Poisoned),
            mem::replace(&mut self.outbound, SubstreamState::Poisoned),
        ) {
            (InEvent::RegisterRequest(reggo), inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::PendingOpen(Message::Register(reggo))),
            ),
            (InEvent::UnregisterRequest { namespace }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::PendingOpen(Message::Unregister { namespace })),
            ),
            (
                InEvent::DiscoverRequest {
                    namespace,
                    cookie,
                    limit,
                },
                inbound,
                SubstreamState::None,
            ) => (
                inbound,
                SubstreamState::Active(Outbound::PendingOpen(Message::Discover {
                    namespace,
                    cookie,
                    limit,
                })),
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
                InEvent::DeclineRegisterRequest(error),
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
            (event, inbound, outbound) => {
                log::warn!(
                    "Neither {:?} nor {:?} can handle {:?}",
                    inbound,
                    outbound,
                    event
                );

                (inbound, outbound)
            }
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
        if let Poll::Ready(event) = self.inbound.poll(cx, &mut ()) {
            return Poll::Ready(event);
        }

        if let Poll::Ready(event) = self.outbound.poll(
            cx,
            &mut OutboundPollParams {
                history: &mut self.outbound_history,
            },
        ) {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
