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

use crate::codec::{
    Cookie, ErrorCode, Message, Namespace, NewRegistration, Registration, RendezvousCodec, Ttl,
};
use crate::handler::{Error, MAX_CONCURRENT_STREAMS};
use crate::handler::PROTOCOL_IDENT;
use crate::substream_handler::{Next, PassthroughProtocol, SubstreamHandler};
use asynchronous_codec::{Framed, RecvSend, Responder};
use futures::{SinkExt, StreamExt};
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::handler::{ConnectionEvent, FullyNegotiatedInbound};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, KeepAlive, NegotiatedSubstream, SubstreamProtocol,
};
use std::fmt;
use std::task::{Context, Poll};
use std::time::Duration;
use timed_streams::TimedStreams;
use void::Void;

pub struct Handler {
    streams: TimedStreams<
        RecvSend<NegotiatedSubstream, RendezvousCodec, asynchronous_codec::CloseStream>,
    >,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            streams: TimedStreams::new(MAX_CONCURRENT_STREAMS, Duration::from_secs(20));
        }
    }
}

impl ConnectionHandler for Handler {
    type InEvent = Void;
    type OutEvent = OutEvent;
    type Error = ();
    type InboundProtocol = ReadyUpgrade<&'static [u8]>;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_IDENT), ())
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        return if self.streams.len() > 0 {
            KeepAlive::Yes
        } else {
            KeepAlive::No
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
                Poll::Ready(Some(Ok(Ok((
                    Message::Discover {
                        namespace,
                        cookie,
                        limit,
                    },
                    responder,
                ))))) => {
                    return Poll::Ready(ConnectionHandlerEvent::Custom(
                        OutEvent::DiscoverRequested {
                            namespace,
                            cookie,
                            limit,
                            responder,
                        },
                    ))
                }
                Poll::Ready(Some(Ok(Ok((Message::Register(new_registration), responder))))) => {
                    return Poll::Ready(ConnectionHandlerEvent::Custom(
                        OutEvent::RegistrationRequested {
                            new_registration,
                            responder,
                        },
                    ))
                }
                Poll::Ready(Some(Ok(Ok((Message::Unregister(namespace), responder))))) => {
                    return Poll::Ready(ConnectionHandlerEvent::Custom(
                        OutEvent::UnregisterRequested {
                            namespace,
                            responder,
                        },
                    ))
                }
                Poll::Ready(Some(Ok(Ok((other, _))))) => {
                    log::debug!("Ignoring invalid message on inbound stream: {message:?}");
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
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn on_behaviour_event(&mut self, event: Self::InEvent) {
        void::unreachable(event);
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
            }) => {
                if let Err(e) = self.streams.try_push(
                    RecvSend::new(protocol, RendezvousCodec::default()).close_after_send(),
                ) {
                    log::debug!("Dropping inbound stream: {e}")
                }
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNEgotiatedOutbound {
                protocol,
                info: (),
            }) => void::unreachable(protocol),
            ConnectionEvent::AddressChange(_) => {}
            ConnectionEvent::DialUpgradeError(_) => {}
            ConnectionEvent::ListenUpgradeError(_) => {}
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone)]
pub enum OutEvent {
    RegistrationRequested {
        new_registration: NewRegistration,
        responder: Responder<Message>,
    },
    UnregisterRequested {
        namespace: Namespace,
        responder: Responder<Message>,
    },
    DiscoverRequested {
        namespace: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<u64>,
        responder: Responder<Message>,
    },
}
