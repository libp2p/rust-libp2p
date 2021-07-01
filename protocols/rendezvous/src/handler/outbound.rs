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
use futures::future::{BoxFuture, Fuse, FusedFuture};
use futures::{FutureExt, SinkExt, TryFutureExt, TryStreamExt};
use libp2p_swarm::NegotiatedSubstream;
use std::task::{Context, Poll};
use void::Void;

pub struct Stream {
    future: Fuse<BoxFuture<'static, Result<OutEvent, Error>>>,
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

impl SubstreamHandler for Stream {
    type InEvent = Void;
    type OutEvent = OutEvent;
    type Error = Error;
    type OpenInfo = OpenInfo;

    fn new(substream: NegotiatedSubstream, info: Self::OpenInfo) -> Self {
        let mut stream = Framed::new(substream, RendezvousCodec::default());
        let sent_message = match info {
            OpenInfo::RegisterRequest(new_registration) => Message::Register(new_registration),
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
        };

        Stream {
            future: async move {
                use Message::*;
                use OutEvent::*;

                stream
                    .send(sent_message.clone())
                    .map_err(Error::WriteMessage)
                    .await?;
                let received_message = stream.try_next().map_err(Error::ReadMessage).await?;
                let received_message = received_message.ok_or(Error::UnexpectedEndOfStream)?;

                let event = match (sent_message, received_message) {
                    (Register(registration), RegisterResponse(Ok(ttl))) => Registered {
                        namespace: registration.namespace.to_owned(),
                        ttl,
                    },
                    (Register(registration), RegisterResponse(Err(error))) => {
                        RegisterFailed(registration.namespace.to_owned(), error)
                    }
                    (Discover { .. }, DiscoverResponse(Ok((registrations, cookie)))) => {
                        Discovered {
                            registrations,
                            cookie,
                        }
                    }
                    (Discover { namespace, .. }, DiscoverResponse(Err(error))) => DiscoverFailed {
                        namespace: namespace.to_owned(),
                        error,
                    },
                    (.., other) => return Err(Error::BadMessage(other)),
                };

                stream.close().map_err(Error::WriteMessage).await?;

                Ok(event)
            }
            .boxed()
            .fuse(),
        }
    }

    fn inject_event(self, event: Self::InEvent) -> Self {
        void::unreachable(event)
    }

    fn advance(mut self, cx: &mut Context<'_>) -> Result<Next<Self, Self::OutEvent>, Self::Error> {
        if self.future.is_terminated() {
            return Ok(Next::Done);
        }

        match self.future.poll_unpin(cx) {
            Poll::Ready(Ok(event)) => Ok(Next::EmitEvent {
                event,
                next_state: self,
            }),
            Poll::Ready(Err(error)) => Err(error),
            Poll::Pending => Ok(Next::Pending { next_state: self }),
        }
    }
}
