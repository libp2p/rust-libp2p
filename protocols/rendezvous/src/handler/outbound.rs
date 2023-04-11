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
use crate::handler::MAX_CONCURRENT_STREAMS;
use crate::handler::PROTOCOL_IDENT;
use crate::{ErrorCode, Namespace, Registration, Ttl};
use asynchronous_codec::SendRecv;
use futures::StreamExt;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::handler::{ConnectionEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, KeepAlive, NegotiatedSubstream, SubstreamProtocol,
};
use std::collections::VecDeque;
use std::task::{Context, Poll};
use std::time::Duration;
use timed_streams::TimedStreams;
use void::Void;

pub struct Handler {
    pending_messages: Vec<Message>,
    inflight_outbound_streams: usize,

    streams: TimedStreams<
        SendRecv<NegotiatedSubstream, RendezvousCodec, asynchronous_codec::CloseStream>,
    >,
    pending_events: VecDeque<OutEvent>,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            pending_messages: Default::default(),
            inflight_outbound_streams: 0,
            streams: TimedStreams::new(MAX_CONCURRENT_STREAMS, Duration::from_secs(20)),
            pending_events: Default::default(),
        }
    }
}

impl Default for Handler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionHandler for Handler {
    type InEvent = InEvent;
    type OutEvent = OutEvent;
    type Error = Void;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = ReadyUpgrade<&'static [u8]>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.streams.is_empty() {
            KeepAlive::No
        } else {
            KeepAlive::Yes
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        loop {
            match self.streams.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(Ok(Message::RegisterResponse(Ok(ttl)))))) => {
                    // TODO: How do we learn here, which registration completed?
                    // Options:
                    // 1. Store a hashmap with ID mapping in `ConnectionHandler`: potentially unbounded memory growth for failed streams (they don't report IDs)
                    // 2. Pass a "context" with `send-recv` that is emitted once it is done
                    // 3. Write it using an async-fn

                    self.pending_events.push_back(OutEvent::Registered {
                        namespace: todo!(),
                        ttl,
                    });
                    continue;
                }
                Poll::Ready(Some(Ok(Ok(Message::RegisterResponse(Err(e)))))) => {
                    self.pending_events
                        .push_back(OutEvent::RegisterFailed(todo!(), e));
                    continue;
                }
                Poll::Ready(Some(Ok(Ok(Message::DiscoverResponse(Ok((
                    registrations,
                    cookie,
                ))))))) => {
                    self.pending_events.push_back(OutEvent::Discovered {
                        registrations,
                        cookie,
                    });
                    continue;
                }
                Poll::Ready(Some(Ok(Ok(Message::DiscoverResponse(Err(e)))))) => {
                    self.pending_events.push_back(OutEvent::DiscoverFailed {
                        namespace: todo!(),
                        error: e,
                    });
                    continue;
                }
                Poll::Ready(Some(Ok(Ok(message)))) => {
                    log::debug!("Ignoring invalid message on outbound stream: {message:?}");
                    continue;
                }
                Poll::Ready(Some(Ok(Err(e)))) => {
                    log::debug!("io failure on inbound stream: {e}");
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    log::debug!("inbound stream timed out: {e}");
                    continue;
                }
                Poll::Ready(None) => {
                    unreachable!("TimedStreams never completes")
                }
                Poll::Pending => break,
            }
        }

        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::Custom(event));
        }

        if self.pending_messages.len() > self.inflight_outbound_streams
            && self.streams.len() + self.inflight_outbound_streams < MAX_CONCURRENT_STREAMS
        {
            self.inflight_outbound_streams += 1;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_IDENT), ()),
            });
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, cmd: Self::InEvent) {
        self.pending_messages.push(match cmd {
            InEvent::RegisterRequest(new_registration) => Message::Register(new_registration),
            InEvent::UnregisterRequest(namespace) => Message::Unregister(namespace),
            InEvent::DiscoverRequest {
                namespace,
                cookie,
                limit,
            } => Message::Discover {
                namespace,
                cookie,
                limit,
            },
        });
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol,
                info: (),
            }) => void::unreachable(protocol),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol,
                info: (),
            }) => {
                self.inflight_outbound_streams -= 1;

                let message = self
                    .pending_messages
                    .pop()
                    .expect("requested an outbound stream without a pending message");
                self.streams
                    .try_push(
                        SendRecv::new(protocol, RendezvousCodec::default(), message)
                            .close_after_send(),
                    )
                    .expect("we never request more than MAX_CONCURRENT_STREAMS");
            }
            ConnectionEvent::AddressChange(_) => {}
            ConnectionEvent::DialUpgradeError(_) => {}
            ConnectionEvent::ListenUpgradeError(_) => {}
        }
    }
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

#[allow(clippy::large_enum_variant)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum InEvent {
    RegisterRequest(NewRegistration),
    UnregisterRequest(Namespace),
    DiscoverRequest {
        namespace: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<Ttl>,
    },
}
