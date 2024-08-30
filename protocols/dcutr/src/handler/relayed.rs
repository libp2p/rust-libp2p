// Copyright 2021 Protocol Labs.
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

//! [`ConnectionHandler`] handling relayed connection potentially upgraded to a direct connection.

use crate::behaviour::MAX_NUMBER_OF_UPGRADE_ATTEMPTS;
use crate::{protocol, PROTOCOL_NAME};
use either::Either;
use futures::future;
use libp2p_core::multiaddr::Multiaddr;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_core::ConnectedPoint;
use libp2p_swarm::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
    ListenUpgradeError,
};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, Stream, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol,
};
use lru::LruCache;
use protocol::{inbound, outbound};
use std::collections::VecDeque;
use std::io;
use std::task::{Context, Poll};
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Debug)]
pub enum Command {
    Connect,
    NewExternalAddrCandidate(Multiaddr),
}

#[derive(Debug)]
pub enum Event {
    InboundConnectNegotiated { remote_addrs: Vec<Multiaddr> },
    OutboundConnectNegotiated { remote_addrs: Vec<Multiaddr> },
    InboundConnectFailed { error: inbound::Error },
    OutboundConnectFailed { error: outbound::Error },
}

pub struct Handler {
    endpoint: ConnectedPoint,
    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,

    // Inbound DCUtR handshakes
    inbound_stream: futures_bounded::FuturesSet<Result<Vec<Multiaddr>, inbound::Error>>,
    pending_inbound_stream: Option<Stream>,

    // Outbound DCUtR handshake.
    outbound_stream: futures_bounded::FuturesSet<Result<Vec<Multiaddr>, outbound::Error>>,

    /// The addresses we will send to the other party for hole-punching attempts.
    holepunch_candidates: LruCache<Multiaddr, ()>,
    pending_outbound_stream: Option<Stream>,

    attempts: u8,
}

impl Handler {
    pub fn new(endpoint: ConnectedPoint, holepunch_candidates: LruCache<Multiaddr, ()>) -> Self {
        Self {
            endpoint,
            queued_events: Default::default(),
            inbound_stream: futures_bounded::FuturesSet::new(Duration::from_secs(10), 1),
            pending_inbound_stream: None,
            outbound_stream: futures_bounded::FuturesSet::new(Duration::from_secs(10), 1),
            pending_outbound_stream: None,
            holepunch_candidates,
            attempts: 0,
        }
    }

    fn set_stream(&mut self, stream_type: StreamType, stream: Stream) {
        if self.holepunch_candidates.is_empty() {
            debug!(
                "New {stream_type} connect stream but holepunch_candidates is empty. Buffering it."
            );
            let has_replaced_old_stream = match stream_type {
                StreamType::Inbound => self.pending_inbound_stream.replace(stream).is_some(),
                StreamType::Outbound => self.pending_outbound_stream.replace(stream).is_some(),
            };
            if has_replaced_old_stream {
                warn!("New {stream_type} connect stream while still buffering previous one. Replacing previous with new.");
            }
        } else {
            let holepunch_candidates = self
                .holepunch_candidates
                .iter()
                .map(|(a, _)| a)
                .cloned()
                .collect();

            let has_replaced_old_stream = match stream_type {
                StreamType::Inbound => {
                    // TODO: when upstreaming this fix, ask libp2p about merging the `attempts` incrementation.
                    self.attempts += 1;

                    // FIXME: actually does not replace !!
                    self.inbound_stream
                        .try_push(inbound::handshake(stream, holepunch_candidates))
                        .is_err()
                }
                StreamType::Outbound => {
                    // FIXME: actually does not replace !!
                    self.outbound_stream
                        .try_push(outbound::handshake(stream, holepunch_candidates))
                        .is_err()
                }
            };

            if has_replaced_old_stream {
                warn!("New {stream_type} connect stream while still upgrading previous one. Replacing previous with new.");
            }
        }
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: output, ..
        }: FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
        match output {
            future::Either::Left(stream) => self.set_stream(StreamType::Inbound, stream),
            // A connection listener denies all incoming substreams, thus none can ever be fully negotiated.
            future::Either::Right(output) => void::unreachable(output),
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: stream, ..
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        assert!(
            self.endpoint.is_listener(),
            "A connection dialer never initiates a connection upgrade."
        );
        self.set_stream(StreamType::Outbound, stream);
    }

    fn on_listen_upgrade_error(
        &mut self,
        ListenUpgradeError { error, .. }: ListenUpgradeError<
            <Self as ConnectionHandler>::InboundOpenInfo,
            <Self as ConnectionHandler>::InboundProtocol,
        >,
    ) {
        void::unreachable(error.into_inner());
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, .. }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        let error = match error {
            StreamUpgradeError::Apply(v) => void::unreachable(v),
            StreamUpgradeError::NegotiationFailed => outbound::Error::Unsupported,
            StreamUpgradeError::Io(e) => outbound::Error::Io(e),
            StreamUpgradeError::Timeout => outbound::Error::Io(io::ErrorKind::TimedOut.into()),
        };

        self.queued_events
            .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                Event::OutboundConnectFailed { error },
            ))
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Command;
    type ToBehaviour = Event;
    type InboundProtocol = Either<ReadyUpgrade<StreamProtocol>, DeniedUpgrade>;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        match self.endpoint {
            ConnectedPoint::Dialer { .. } => {
                SubstreamProtocol::new(Either::Left(ReadyUpgrade::new(PROTOCOL_NAME)), ())
            }
            ConnectedPoint::Listener { .. } => {
                // By the protocol specification the listening side of a relayed connection
                // initiates the _direct connection upgrade_. In other words the listening side of
                // the relayed connection opens a substream to the dialing side. (Connection roles
                // and substream roles are reversed.) The listening side on a relayed connection
                // never expects incoming substreams, hence the denied upgrade below.
                SubstreamProtocol::new(Either::Right(DeniedUpgrade), ())
            }
        }
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            Command::Connect => {
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_NAME), ()),
                    });

                // TODO: when upstreaming this fix, ask libp2p about merging the `attempts` incrementation.
                // Indeed, even if the `queued_event` above will be trigger on stream opening, it might not start
                self.attempts += 1;
            }
            Command::NewExternalAddrCandidate(address) => {
                self.holepunch_candidates.push(address, ());

                if let Some(stream) = self.pending_inbound_stream.take() {
                    self.set_stream(StreamType::Inbound, stream);
                }

                if let Some(stream) = self.pending_outbound_stream.take() {
                    self.set_stream(StreamType::Outbound, stream);
                }
            }
        }
    }

    fn connection_keep_alive(&self) -> bool {
        if self.attempts < MAX_NUMBER_OF_UPGRADE_ATTEMPTS {
            return true;
        }

        false
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        // Return queued events.
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        match self.inbound_stream.poll_unpin(cx) {
            Poll::Ready(Ok(Ok(addresses))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::InboundConnectNegotiated {
                        remote_addrs: addresses,
                    },
                ))
            }
            Poll::Ready(Ok(Err(error))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::InboundConnectFailed { error },
                ))
            }
            Poll::Ready(Err(futures_bounded::Timeout { .. })) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::InboundConnectFailed {
                        error: inbound::Error::Io(io::ErrorKind::TimedOut.into()),
                    },
                ))
            }
            Poll::Pending => {}
        }

        match self.outbound_stream.poll_unpin(cx) {
            Poll::Ready(Ok(Ok(addresses))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundConnectNegotiated {
                        remote_addrs: addresses,
                    },
                ))
            }
            Poll::Ready(Ok(Err(error))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundConnectFailed { error },
                ))
            }
            Poll::Ready(Err(futures_bounded::Timeout { .. })) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundConnectFailed {
                        error: outbound::Error::Io(io::ErrorKind::TimedOut.into()),
                    },
                ))
            }
            Poll::Pending => {}
        }

        Poll::Pending
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
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            _ => {}
        }
    }
}

enum StreamType {
    Inbound,
    Outbound,
}

impl std::fmt::Display for StreamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inbound => write!(f, "inbound"),
            Self::Outbound => write!(f, "outbound"),
        }
    }
}
