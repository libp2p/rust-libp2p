use crate::codec::{
    self, Challenge, Cookie, ErrorCode, Message, NewRegistration, RegisterErrorResponse,
    Registration, RendezvousCodec,
};
use crate::pow::Difficulty;
use crate::substream::{Advance, Next, SubstreamState};
use crate::{pow, protocol};
use asynchronous_codec::Framed;
use futures::{SinkExt, StreamExt};
use libp2p_core::{InboundUpgrade, OutboundUpgrade};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use std::fmt::Debug;
use std::mem;
use std::sync::mpsc::TryRecvError;
use std::task::{Context, Poll};
use void::Void;

pub struct RendezvousHandler {
    outbound: SubstreamState<Outbound>,
    outbound_history: MessageHistory,
    inbound: SubstreamState<Inbound>,
    inbound_history: MessageHistory,
    max_difficulty: Difficulty,
}

impl RendezvousHandler {
    pub fn new(max_difficulty: Difficulty) -> Self {
        Self {
            outbound: SubstreamState::None,
            outbound_history: Default::default(),
            max_difficulty,
            inbound: SubstreamState::None,
            inbound_history: Default::default(),
        }
    }
}

#[derive(Default)]
struct MessageHistory {
    sent: Vec<Message>,
    received: Vec<Message>,
}

impl MessageHistory {
    fn clear(&mut self) {
        self.sent.clear();
        self.received.clear();
    }
}

#[derive(Debug, Clone)]
pub enum OutEvent {
    /// A peer wants to store registration with us.
    RegistrationRequested {
        registration: NewRegistration,
        /// The PoW that was supplied with the registration.
        pow_difficulty: Difficulty,
    },
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
    DeclineRegisterRequest(DeclineReason),
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

#[derive(Debug)]
pub enum DeclineReason {
    BadRegistration(ErrorCode),
    PowRequired { target: Difficulty },
}

/// The state of an inbound substream (i.e. the remote node opened it).
enum Inbound {
    /// We are in the process of reading a message from the substream.
    PendingRead(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We read a message, dispatched it to the behaviour and are waiting for the response.
    PendingBehaviour(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We are in the process of sending a response.
    PendingSend(Framed<NegotiatedSubstream, RendezvousCodec>, Message),
    /// We started sending and are currently flushing the data out, afterwards we will go and read the next message.
    PendingFlushThenRead(Framed<NegotiatedSubstream, RendezvousCodec>),
    /// We've sent the message and are now closing down the substream.
    PendingClose(Framed<NegotiatedSubstream, RendezvousCodec>),
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
    /// We are waiting for our PoW thread to finish.
    PendingPoW {
        substream: Framed<NegotiatedSubstream, RendezvousCodec>,
        channel: std::sync::mpsc::Receiver<Result<([u8; 32], i64), pow::ExhaustedNonceSpace>>,
    },
    /// We are waiting for the response from the remote.
    PendingRemote(Framed<NegotiatedSubstream, RendezvousCodec>),
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
    #[error("Failed to compute proof of work")]
    PowFailed(#[from] pow::ExhaustedNonceSpace),
    #[error("Failed to verify Proof of Work")]
    BadPoWSupplied(#[from] pow::VerifyError),
    #[error("Rendezvous point requested difficulty {requested} but we are only willing to produce {limit}")]
    MaxDifficultyExceeded {
        requested: Difficulty,
        limit: Difficulty,
    },
}

struct InboundPollParams<'handler> {
    history: &'handler mut MessageHistory,
}

impl<'handler> Advance<'handler> for Inbound {
    type Event = OutEvent;
    type Params = InboundPollParams<'handler>;
    type Error = Error;
    type Protocol = SubstreamProtocol<protocol::Rendezvous, Message>;

    fn advance(
        self,
        cx: &mut Context<'_>,
        InboundPollParams { history }: &mut Self::Params,
    ) -> Result<Next<Self, Self::Event, Self::Protocol>, Self::Error> {
        Ok(match self {
            Inbound::PendingRead(mut substream) => match substream
                .poll_next_unpin(cx)
                .map_err(Error::ReadMessage)?
            {
                Poll::Ready(Some(msg)) => {
                    let event = match (
                        history.received.as_slice(),
                        history.sent.as_slice(),
                        msg.clone(),
                    ) {
                        (.., Message::Register(registration)) => OutEvent::RegistrationRequested {
                            registration,
                            pow_difficulty: Difficulty::ZERO, // initial Register has no PoW
                        },
                        // this next pattern matches if:
                        // 1. the first message we received from this peer was `Register`
                        // 2. the last message we sent to them was `PowRequired`
                        // 3. the message we just received is `ProofOfWork`
                        (
                            [Message::Register(registration), ..],
                            [.., Message::RegisterResponse(Err(RegisterErrorResponse::PowRequired {
                                challenge,
                                target: target_difficulty,
                            }))],
                            Message::ProofOfWork { hash, nonce },
                        ) => {
                            pow::verify(
                                challenge.as_bytes(),
                                registration.namespace.as_str(),
                                registration.record.to_signed_envelope(),
                                *target_difficulty,
                                hash,
                                nonce,
                            )?;

                            OutEvent::RegistrationRequested {
                                registration: registration.clone(),
                                pow_difficulty: pow::difficulty_of(&hash),
                            }
                        }
                        (.., Message::Discover { cookie, namespace }) => {
                            OutEvent::DiscoverRequested { cookie, namespace }
                        }
                        (.., Message::Unregister { namespace }) => {
                            OutEvent::UnregisterRequested { namespace }
                        }
                        (.., other) => return Err(Error::BadMessage(other)),
                    };

                    history.received.push(msg);

                    Next::EmitEvent {
                        event,
                        next_state: Inbound::PendingBehaviour(substream),
                    }
                }
                Poll::Ready(None) => return Err(Error::UnexpectedEndOfStream),
                Poll::Pending => Next::Pending {
                    next_state: Inbound::PendingRead(substream),
                },
            },
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

                    let next = match message {
                        // In case we requested PoW from the client, we need to wait for the response and hence go to `PendingFlushThenRead` afterwards
                        Message::RegisterResponse(Err(RegisterErrorResponse::PowRequired {
                            ..
                        })) => Next::Continue {
                            next_state: Inbound::PendingFlushThenRead(substream),
                        },
                        // In case of any other message, just close the stream (that implies flushing)
                        _ => Next::Continue {
                            next_state: Inbound::PendingClose(substream),
                        },
                    };

                    history.sent.push(message);

                    next
                }
                Poll::Pending => Next::Pending {
                    next_state: Inbound::PendingSend(substream, message),
                },
            },
            Inbound::PendingFlushThenRead(mut substream) => {
                match substream
                    .poll_flush_unpin(cx)
                    .map_err(Error::WriteMessage)?
                {
                    Poll::Ready(()) => Next::Continue {
                        next_state: Inbound::PendingRead(substream),
                    },
                    Poll::Pending => Next::Pending {
                        next_state: Inbound::PendingFlushThenRead(substream),
                    },
                }
            }
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
    max_difficulty: Difficulty,
}

impl<'handler> Advance<'handler> for Outbound {
    type Event = OutEvent;
    type Params = OutboundPollParams<'handler>;
    type Error = Error;
    type Protocol = SubstreamProtocol<protocol::Rendezvous, Message>;

    fn advance(
        self,
        cx: &mut Context<'_>,
        OutboundPollParams {
            history,
            max_difficulty,
        }: &mut OutboundPollParams,
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
                        (
                            [Register(registration), ..],
                            RegisterResponse(Err(RegisterErrorResponse::Failed(error))),
                        ) => RegisterFailed {
                            namespace: registration.namespace.to_owned(),
                            error,
                        },
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
                        (
                            [Register(registration), ..],
                            RegisterResponse(Err(RegisterErrorResponse::PowRequired {
                                challenge,
                                target: target_difficulty,
                            })),
                        ) => {
                            if target_difficulty > *max_difficulty {
                                return Err(Error::MaxDifficultyExceeded {
                                    requested: target_difficulty,
                                    limit: *max_difficulty,
                                });
                            }

                            let (sender, receiver) = std::sync::mpsc::channel();

                            // do the PoW on a separate thread to not block the networking tasks
                            std::thread::spawn({
                                let waker = cx.waker().clone();
                                let registration = registration.clone();

                                move || {
                                    let result = pow::run(
                                        challenge.as_bytes(),
                                        &registration.namespace,
                                        registration.record.to_signed_envelope(),
                                        target_difficulty,
                                    );

                                    waker.wake(); // let the runtime know that we are ready, this should get us polled

                                    let _ = sender.send(result);
                                }
                            });

                            return Ok(Next::Continue {
                                next_state: Outbound::PendingPoW {
                                    substream,
                                    channel: receiver,
                                },
                            });
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
            Outbound::PendingPoW { substream, channel } => {
                let (hash, nonce) = match channel.try_recv() {
                    Ok(result) => result?,
                    Err(TryRecvError::Empty) => {
                        return Ok(Next::Pending {
                            next_state: Outbound::PendingPoW { substream, channel },
                        })
                    }
                    Err(TryRecvError::Disconnected) => {
                        unreachable!("sender is never dropped")
                    }
                };

                return Ok(Next::Continue {
                    next_state: Outbound::PendingSend {
                        substream,
                        to_send: Message::ProofOfWork { hash, nonce },
                    },
                });
            }
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
                self.inbound_history.clear();
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
            (InEvent::RegisterRequest { request: reggo }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::PendingOpen(Message::Register(reggo))),
            ),
            (InEvent::UnregisterRequest { namespace }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::PendingOpen(Message::Unregister { namespace })),
            ),
            (InEvent::DiscoverRequest { namespace, cookie }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::PendingOpen(Message::Discover { namespace, cookie })),
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
                InEvent::DeclineRegisterRequest(DeclineReason::BadRegistration(error)),
                SubstreamState::Active(Inbound::PendingBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::RegisterResponse(Err(RegisterErrorResponse::Failed(error))),
                )),
                outbound,
            ),
            (
                InEvent::DeclineRegisterRequest(DeclineReason::PowRequired {
                    target: target_difficulty,
                }),
                SubstreamState::Active(Inbound::PendingBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::RegisterResponse(Err(RegisterErrorResponse::PowRequired {
                        challenge: Challenge::new(&mut rand::thread_rng()),
                        target: target_difficulty,
                    })),
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
        if let Poll::Ready(event) = self.inbound.poll(
            cx,
            &mut InboundPollParams {
                history: &mut self.inbound_history,
            },
        ) {
            return Poll::Ready(event);
        }

        if let Poll::Ready(event) = self.outbound.poll(
            cx,
            &mut OutboundPollParams {
                history: &mut self.outbound_history,
                max_difficulty: self.max_difficulty,
            },
        ) {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
