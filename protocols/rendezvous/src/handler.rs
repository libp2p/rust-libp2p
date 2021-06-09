use crate::codec::{ErrorCode, Message, Registration};
use crate::codec::{NewRegistration, RendezvousCodec};
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
    outbound: SubstreamState<Outbound>,
    inbound: SubstreamState<Inbound>,
    keep_alive: KeepAlive,
}

impl RendezvousHandler {
    pub fn new() -> Self {
        Self {
            outbound: SubstreamState::None,
            inbound: SubstreamState::None,
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
    DeclineRegisterRequest {
        error: ErrorCode,
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

#[derive(Debug)]
enum SubstreamState<S> {
    /// There is no substream.
    None,
    /// The substream is in an active state.
    Active(S),
    /// Something went seriously wrong.
    Poisoned,
}

/// Advances a state machine.
trait Advance: Sized {
    type Event;

    fn advance(self, cx: &mut Context<'_>) -> Next<Self, Self::Event>;
}

/// Defines the results of advancing a state machine.
enum Next<S, E> {
    /// Return from the `poll` function, either because are `Ready` or there is no more work to do (`Pending`).
    Return { poll: Poll<E>, next_state: S },
    /// Continue with polling the state.
    Continue { next_state: S },
    /// The state machine finished.
    Done,
}

impl<S, E> SubstreamState<S>
where
    S: Advance<Event = E>,
{
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<E> {
        loop {
            let next = match mem::replace(self, SubstreamState::Poisoned) {
                SubstreamState::None => {
                    *self = SubstreamState::None;
                    return Poll::Pending;
                }
                SubstreamState::Active(state) => state.advance(cx),
                SubstreamState::Poisoned => {
                    unreachable!("reached poisoned state")
                }
            };

            match next {
                Next::Continue { next_state } => {
                    *self = SubstreamState::Active(next_state);
                    continue;
                }
                Next::Return { poll, next_state } => {
                    *self = SubstreamState::Active(next_state);
                    return poll;
                }
                Next::Done => {
                    *self = SubstreamState::None;
                    return Poll::Pending;
                }
            }
        }
    }
}

/// The state of an inbound substream (i.e. the remote node opened it).
enum Inbound {
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
}

/// The state of an outbound substream (i.e. we opened it).
enum Outbound {
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
}

impl Advance for Inbound {
    type Event = ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, codec::Error>;

    fn advance(
        self,
        cx: &mut Context<'_>,
    ) -> Next<Self, ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, codec::Error>>
    {
        match self {
            Inbound::Reading(mut substream) => match substream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    debug!("read message from inbound {:?}", msg);

                    if let Message::Register(..)
                    | Message::Discover { .. }
                    | Message::Unregister { .. } = msg
                    {
                        Next::Return {
                            poll: Poll::Ready(ProtocolsHandlerEvent::Custom(msg)),
                            next_state: Inbound::WaitForBehaviour(substream),
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
                    next_state: Inbound::Reading(substream),
                },
            },
            Inbound::WaitForBehaviour(substream) => Next::Return {
                poll: Poll::Pending,
                next_state: Inbound::WaitForBehaviour(substream),
            },
            Inbound::PendingSend(mut substream, message) => match substream.poll_ready_unpin(cx) {
                Poll::Ready(Ok(())) => match substream.start_send_unpin(message) {
                    Ok(()) => Next::Continue {
                        next_state: Inbound::PendingFlush(substream),
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
                    next_state: Inbound::PendingSend(substream, message),
                },
            },
            Inbound::PendingFlush(mut substream) => match substream.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Continue {
                    next_state: Inbound::Closing(substream),
                },
                Poll::Ready(Err(e)) => panic!("pending send from inbound error: {:?}", e),
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Inbound::PendingFlush(substream),
                },
            },
            Inbound::Closing(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(..) => Next::Done,
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Inbound::Closing(substream),
                },
            },
        }
    }
}

impl Advance for Outbound {
    type Event = ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, codec::Error>;

    fn advance(
        self,
        cx: &mut Context<'_>,
    ) -> Next<Self, ProtocolsHandlerEvent<protocol::Rendezvous, Message, OutEvent, codec::Error>>
    {
        match self {
            Outbound::Start(msg) => Next::Return {
                poll: Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(protocol::new(), msg),
                }),
                next_state: Outbound::WaitingUpgrade,
            },
            Outbound::WaitingUpgrade => Next::Return {
                poll: Poll::Pending,
                next_state: Outbound::WaitingUpgrade,
            },
            Outbound::PendingSend(mut substream, message) => match substream.poll_ready_unpin(cx) {
                Poll::Ready(Ok(())) => match substream.start_send_unpin(message) {
                    Ok(()) => Next::Continue {
                        next_state: Outbound::PendingFlush(substream),
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
                    next_state: Outbound::PendingSend(substream, message),
                },
            },
            Outbound::PendingFlush(mut substream) => match substream.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Continue {
                    next_state: Outbound::WaitForRemote(substream),
                },
                Poll::Ready(Err(e)) => {
                    panic!("Error when flushing outbound: {:?}", e);
                }
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Outbound::PendingFlush(substream),
                },
            },
            Outbound::WaitForRemote(mut substream) => match substream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(msg))) => {
                    if let Message::DiscoverResponse { .. }
                    | Message::RegisterResponse { .. }
                    | Message::FailedToDiscover { .. }
                    | Message::FailedToRegister { .. } = msg
                    {
                        Next::Return {
                            poll: Poll::Ready(ProtocolsHandlerEvent::Custom(msg)),
                            next_state: Outbound::Closing(substream),
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
                    next_state: Outbound::WaitForRemote(substream),
                },
            },
            Outbound::Closing(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(..) => Next::Done,
                Poll::Pending => Next::Return {
                    poll: Poll::Pending,
                    next_state: Outbound::Closing(substream),
                },
            },
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
        SubstreamProtocol::new(protocol::new(), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _msg: Self::InboundOpenInfo,
    ) {
        debug!("injected inbound");
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
        debug!("injected outbound");
        if let SubstreamState::Active(Outbound::WaitingUpgrade) = self.outbound {
            self.outbound = SubstreamState::Active(Outbound::PendingSend(substream, msg));
        } else {
            unreachable!("Invalid outbound state")
        }
    }

    // event injected from NotifyHandler
    fn inject_event(&mut self, req: InEvent) {
        debug!("injecting event into handler from behaviour: {:?}", &req);
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
            (InEvent::DiscoverRequest { namespace }, inbound, SubstreamState::None) => (
                inbound,
                SubstreamState::Active(Outbound::Start(Message::Discover { namespace })),
            ),
            (
                InEvent::RegisterResponse { ttl },
                SubstreamState::Active(Inbound::WaitForBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::RegisterResponse { ttl },
                )),
                outbound,
            ),
            (
                InEvent::DeclineRegisterRequest { error },
                SubstreamState::Active(Inbound::WaitForBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::FailedToRegister { error },
                )),
                outbound,
            ),
            (
                InEvent::DiscoverResponse { discovered },
                SubstreamState::Active(Inbound::WaitForBehaviour(substream)),
                outbound,
            ) => (
                SubstreamState::Active(Inbound::PendingSend(
                    substream,
                    Message::DiscoverResponse {
                        registrations: discovered,
                    },
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

impl Debug for Outbound {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Outbound::Start(_) => f.write_str("start"),
            Outbound::WaitingUpgrade => f.write_str("waiting_upgrade"),
            Outbound::PendingSend(_, _) => f.write_str("pending_send"),
            Outbound::PendingFlush(_) => f.write_str("pending_flush"),
            Outbound::WaitForRemote(_) => f.write_str("waiting_for_remote"),
            Outbound::Closing(_) => f.write_str("closing"),
        }
    }
}

impl Debug for Inbound {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Inbound::Reading(_) => f.write_str("reading"),
            Inbound::PendingSend(_, _) => f.write_str("pending_send"),
            Inbound::PendingFlush(_) => f.write_str("pending_flush"),
            Inbound::WaitForBehaviour(_) => f.write_str("waiting_for_behaviour"),
            Inbound::Closing(_) => f.write_str("closing"),
        }
    }
}
