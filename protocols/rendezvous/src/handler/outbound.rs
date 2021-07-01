// Copyright 2021 COMIT Network.
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

use crate::codec::{Cookie, Message, NewRegistration, RendezvousCodec};
use crate::handler::Error;
use crate::substream_handler::{Next, SubstreamHandler};
use crate::{ErrorCode, Namespace, Registration, Ttl};
use asynchronous_codec::Framed;
use futures::{SinkExt, StreamExt};
use libp2p_swarm::NegotiatedSubstream;
use std::task::{Context, Poll};
use void::Void;

pub struct Stream {
    history: MessageHistory,
    state: State,
}

#[derive(Default)]
struct MessageHistory {
    sent: Vec<Message>,
}

#[derive(Debug, Clone)]
pub enum OutEvent {
    Registered {
        namespace: Namespace,
        ttl: Ttl,
    },
    RegisterFailed(Namespace, ErrorCode),
    Discovered {
        registrations: Vec<Registration>,
        cookie: Cookie,
    },
    DiscoverFailed {
        namespace: Option<Namespace>,
        error: ErrorCode,
    },
}

#[derive(Debug)]
pub enum OpenInfo {
    RegisterRequest(NewRegistration),
    UnregisterRequest(Namespace),
    DiscoverRequest {
        namespace: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<Ttl>,
    },
}

/// The state of an outbound substream (i.e. we opened it).
enum State {
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

impl SubstreamHandler for Stream {
    type InEvent = Void;
    type OutEvent = OutEvent;
    type Error = Error;
    type OpenInfo = OpenInfo;

    fn new(substream: NegotiatedSubstream, info: Self::OpenInfo) -> Self {
        Stream {
            history: Default::default(),
            state: State::PendingSend {
                substream: Framed::new(substream, RendezvousCodec::default()),
                to_send: match info {
                    OpenInfo::RegisterRequest(new_registration) => {
                        Message::Register(new_registration)
                    }
                    OpenInfo::UnregisterRequest(namespace) => Message::Unregister(namespace),
                    OpenInfo::DiscoverRequest {
                        namespace,
                        cookie,
                        limit,
                    } => Message::Discover {
                        namespace,
                        cookie,
                        limit,
                    },
                },
            },
        }
    }

    fn inject_event(self, event: Self::InEvent) -> Self {
        void::unreachable(event)
    }

    fn advance(self, cx: &mut Context<'_>) -> Result<Next<Self, Self::OutEvent>, Self::Error> {
        let Stream { state, mut history } = self;

        let next = match state {
            State::PendingSend {
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
                        next_state: State::PendingFlush(substream),
                    }
                }
                Poll::Pending => Next::Pending {
                    next_state: State::PendingSend {
                        substream,
                        to_send: message,
                    },
                },
            },
            State::PendingFlush(mut substream) => match substream
                .poll_flush_unpin(cx)
                .map_err(Error::WriteMessage)?
            {
                Poll::Ready(()) => Next::Continue {
                    next_state: State::PendingRemote(substream),
                },
                Poll::Pending => Next::Pending {
                    next_state: State::PendingFlush(substream),
                },
            },
            State::PendingRemote(mut substream) => match substream
                .poll_next_unpin(cx)
                .map_err(Error::ReadMessage)?
            {
                Poll::Ready(Some(received_message)) => {
                    use crate::codec::Message::*;
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
                            RegisterFailed(registration.namespace.to_owned(), error)
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
                        next_state: State::PendingClose(substream),
                    }
                }
                Poll::Ready(None) => return Err(Error::UnexpectedEndOfStream),
                Poll::Pending => Next::Pending {
                    next_state: State::PendingRemote(substream),
                },
            },
            State::PendingClose(mut substream) => match substream.poll_close_unpin(cx) {
                Poll::Ready(Ok(())) => Next::Done,
                Poll::Ready(Err(_)) => Next::Done, // there is nothing we can do about an error during close
                Poll::Pending => Next::Pending {
                    next_state: State::PendingClose(substream),
                },
            },
        };
        let next = next.map_state(|state| Stream { history, state });

        Ok(next)
    }
}
