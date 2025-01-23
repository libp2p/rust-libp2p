// Copyright 2020 Parity Technologies (UK) Ltd.
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

pub(crate) mod protocol;

use std::{
    collections::VecDeque,
    fmt, io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent, DialUpgradeError,
        FullyNegotiatedInbound, FullyNegotiatedOutbound, ListenUpgradeError, StreamUpgradeError,
    },
    SubstreamProtocol,
};
pub use protocol::ProtocolSupport;
use smallvec::SmallVec;

use crate::{
    codec::Codec, handler::protocol::Protocol, InboundRequestId, OutboundRequestId,
    EMPTY_QUEUE_SHRINK_THRESHOLD,
};

/// A connection handler for a request response [`Behaviour`](super::Behaviour) protocol.
pub struct Handler<TCodec>
where
    TCodec: Codec,
{
    /// The supported inbound protocols.
    inbound_protocols: SmallVec<[TCodec::Protocol; 2]>,
    /// The request/response message codec.
    codec: TCodec,
    /// Queue of events to emit in `poll()`.
    pending_events: VecDeque<Event<TCodec>>,
    /// Outbound upgrades waiting to be emitted as an `OutboundSubstreamRequest`.
    pending_outbound: VecDeque<OutboundMessage<TCodec>>,

    requested_outbound: VecDeque<OutboundMessage<TCodec>>,
    /// A channel for receiving inbound requests.
    inbound_receiver: mpsc::Receiver<(
        InboundRequestId,
        TCodec::Request,
        oneshot::Sender<TCodec::Response>,
    )>,
    /// The [`mpsc::Sender`] for the above receiver. Cloned for each inbound request.
    inbound_sender: mpsc::Sender<(
        InboundRequestId,
        TCodec::Request,
        oneshot::Sender<TCodec::Response>,
    )>,

    inbound_request_id: Arc<AtomicU64>,

    worker_streams: futures_bounded::FuturesMap<RequestId, Result<Event<TCodec>, io::Error>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RequestId {
    Inbound(InboundRequestId),
    Outbound(OutboundRequestId),
}

impl<TCodec> Handler<TCodec>
where
    TCodec: Codec + Send + Clone + 'static,
{
    pub(super) fn new(
        inbound_protocols: SmallVec<[TCodec::Protocol; 2]>,
        codec: TCodec,
        substream_timeout: Duration,
        inbound_request_id: Arc<AtomicU64>,
        max_concurrent_streams: usize,
    ) -> Self {
        let (inbound_sender, inbound_receiver) = mpsc::channel(0);
        Self {
            inbound_protocols,
            codec,
            pending_outbound: VecDeque::new(),
            requested_outbound: Default::default(),
            inbound_receiver,
            inbound_sender,
            pending_events: VecDeque::new(),
            inbound_request_id,
            worker_streams: futures_bounded::FuturesMap::new(
                substream_timeout,
                max_concurrent_streams,
            ),
        }
    }

    /// Returns the next inbound request ID.
    fn next_inbound_request_id(&mut self) -> InboundRequestId {
        InboundRequestId(self.inbound_request_id.fetch_add(1, Ordering::Relaxed))
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: (mut stream, protocol),
            info: (),
        }: FullyNegotiatedInbound<<Self as ConnectionHandler>::InboundProtocol>,
    ) {
        let mut codec = self.codec.clone();
        let request_id = self.next_inbound_request_id();
        let mut sender = self.inbound_sender.clone();

        let recv = async move {
            // A channel for notifying the inbound upgrade when the
            // response is sent.
            let (rs_send, rs_recv) = oneshot::channel();

            let read = codec.read_request(&protocol, &mut stream);
            let request = read.await?;
            sender
                .send((request_id, request, rs_send))
                .await
                .expect("`ConnectionHandler` owns both ends of the channel");
            drop(sender);

            if let Ok(response) = rs_recv.await {
                let write = codec.write_response(&protocol, &mut stream, response);
                write.await?;

                stream.close().await?;
                Ok(Event::ResponseSent(request_id))
            } else {
                stream.close().await?;
                Ok(Event::ResponseOmission(request_id))
            }
        };

        // Inbound connections are reported to the upper layer from within the above task,
        // so by failing to schedule it, it means the upper layer will never know about the
        // inbound request. Because of that we do not report any inbound failure.
        if self
            .worker_streams
            .try_push(RequestId::Inbound(request_id), recv.boxed())
            .is_err()
        {
            tracing::warn!("Dropping inbound stream because we are at capacity")
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: (mut stream, protocol),
            info: (),
        }: FullyNegotiatedOutbound<<Self as ConnectionHandler>::OutboundProtocol>,
    ) {
        let message = self
            .requested_outbound
            .pop_front()
            .expect("negotiated a stream without a pending message");

        let mut codec = self.codec.clone();
        let request_id = message.request_id;

        let send = async move {
            let write = codec.write_request(&protocol, &mut stream, message.request);
            write.await?;
            stream.close().await?;
            let read = codec.read_response(&protocol, &mut stream);
            let response = read.await?;

            Ok(Event::Response {
                request_id,
                response,
            })
        };

        if self
            .worker_streams
            .try_push(RequestId::Outbound(request_id), send.boxed())
            .is_err()
        {
            self.pending_events.push_back(Event::OutboundStreamFailed {
                request_id: message.request_id,
                error: io::Error::new(io::ErrorKind::Other, "max sub-streams reached"),
            });
        }
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, info: () }: DialUpgradeError<
            (),
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        let message = self
            .requested_outbound
            .pop_front()
            .expect("negotiated a stream without a pending message");

        match error {
            StreamUpgradeError::Timeout => {
                self.pending_events
                    .push_back(Event::OutboundTimeout(message.request_id));
            }
            StreamUpgradeError::NegotiationFailed => {
                // The remote merely doesn't support the protocol(s) we requested.
                // This is no reason to close the connection, which may
                // successfully communicate with other protocols already.
                // An event is reported to permit user code to react to the fact that
                // the remote peer does not support the requested protocol(s).
                self.pending_events
                    .push_back(Event::OutboundUnsupportedProtocols(message.request_id));
            }
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            StreamUpgradeError::Apply(e) => libp2p_core::util::unreachable(e),
            StreamUpgradeError::Io(e) => {
                self.pending_events.push_back(Event::OutboundStreamFailed {
                    request_id: message.request_id,
                    error: e,
                });
            }
        }
    }
    fn on_listen_upgrade_error(
        &mut self,
        ListenUpgradeError { error, .. }: ListenUpgradeError<
            (),
            <Self as ConnectionHandler>::InboundProtocol,
        >,
    ) {
        // TODO: remove when Rust 1.82 is MSRV
        #[allow(unreachable_patterns)]
        libp2p_core::util::unreachable(error)
    }
}

/// The events emitted by the [`Handler`].
pub enum Event<TCodec>
where
    TCodec: Codec,
{
    /// A request has been received.
    Request {
        request_id: InboundRequestId,
        request: TCodec::Request,
        sender: oneshot::Sender<TCodec::Response>,
    },
    /// A response has been received.
    Response {
        request_id: OutboundRequestId,
        response: TCodec::Response,
    },
    /// A response to an inbound request has been sent.
    ResponseSent(InboundRequestId),
    /// A response to an inbound request was omitted as a result
    /// of dropping the response `sender` of an inbound `Request`.
    ResponseOmission(InboundRequestId),
    /// An outbound request timed out while sending the request
    /// or waiting for the response.
    OutboundTimeout(OutboundRequestId),
    /// An outbound request failed to negotiate a mutually supported protocol.
    OutboundUnsupportedProtocols(OutboundRequestId),
    OutboundStreamFailed {
        request_id: OutboundRequestId,
        error: io::Error,
    },
    /// An inbound request timed out while waiting for the request
    /// or sending the response.
    InboundTimeout(InboundRequestId),
    InboundStreamFailed {
        request_id: InboundRequestId,
        error: io::Error,
    },
}

impl<TCodec: Codec> fmt::Debug for Event<TCodec> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request {
                request_id,
                request: _,
                sender: _,
            } => f
                .debug_struct("Event::Request")
                .field("request_id", request_id)
                .finish(),
            Event::Response {
                request_id,
                response: _,
            } => f
                .debug_struct("Event::Response")
                .field("request_id", request_id)
                .finish(),
            Event::ResponseSent(request_id) => f
                .debug_tuple("Event::ResponseSent")
                .field(request_id)
                .finish(),
            Event::ResponseOmission(request_id) => f
                .debug_tuple("Event::ResponseOmission")
                .field(request_id)
                .finish(),
            Event::OutboundTimeout(request_id) => f
                .debug_tuple("Event::OutboundTimeout")
                .field(request_id)
                .finish(),
            Event::OutboundUnsupportedProtocols(request_id) => f
                .debug_tuple("Event::OutboundUnsupportedProtocols")
                .field(request_id)
                .finish(),
            Event::OutboundStreamFailed { request_id, error } => f
                .debug_struct("Event::OutboundStreamFailed")
                .field("request_id", &request_id)
                .field("error", &error)
                .finish(),
            Event::InboundTimeout(request_id) => f
                .debug_tuple("Event::InboundTimeout")
                .field(request_id)
                .finish(),
            Event::InboundStreamFailed { request_id, error } => f
                .debug_struct("Event::InboundStreamFailed")
                .field("request_id", &request_id)
                .field("error", &error)
                .finish(),
        }
    }
}

pub struct OutboundMessage<TCodec: Codec> {
    pub(crate) request_id: OutboundRequestId,
    pub(crate) request: TCodec::Request,
    pub(crate) protocols: SmallVec<[TCodec::Protocol; 2]>,
}

impl<TCodec> fmt::Debug for OutboundMessage<TCodec>
where
    TCodec: Codec,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutboundMessage").finish_non_exhaustive()
    }
}

impl<TCodec> ConnectionHandler for Handler<TCodec>
where
    TCodec: Codec + Send + Clone + 'static,
{
    type FromBehaviour = OutboundMessage<TCodec>;
    type ToBehaviour = Event<TCodec>;
    type InboundProtocol = Protocol<TCodec::Protocol>;
    type OutboundProtocol = Protocol<TCodec::Protocol>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(
            Protocol {
                protocols: self.inbound_protocols.clone(),
            },
            (),
        )
    }

    fn on_behaviour_event(&mut self, request: Self::FromBehaviour) {
        self.pending_outbound.push_back(request);
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Protocol<TCodec::Protocol>, (), Self::ToBehaviour>> {
        match self.worker_streams.poll_unpin(cx) {
            Poll::Ready((_, Ok(Ok(event)))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
            }
            Poll::Ready((RequestId::Inbound(id), Ok(Err(e)))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::InboundStreamFailed {
                        request_id: id,
                        error: e,
                    },
                ));
            }
            Poll::Ready((RequestId::Outbound(id), Ok(Err(e)))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundStreamFailed {
                        request_id: id,
                        error: e,
                    },
                ));
            }
            Poll::Ready((RequestId::Inbound(id), Err(futures_bounded::Timeout { .. }))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::InboundTimeout(id),
                ));
            }
            Poll::Ready((RequestId::Outbound(id), Err(futures_bounded::Timeout { .. }))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundTimeout(id),
                ));
            }
            Poll::Pending => {}
        }

        // Drain pending events that were produced by `worker_streams`.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        } else if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }

        // Check for inbound requests.
        if let Poll::Ready(Some((id, rq, rs_sender))) = self.inbound_receiver.poll_next_unpin(cx) {
            // We received an inbound request.

            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event::Request {
                request_id: id,
                request: rq,
                sender: rs_sender,
            }));
        }

        // Emit outbound requests.
        if let Some(request) = self.pending_outbound.pop_front() {
            let protocols = request.protocols.clone();
            self.requested_outbound.push_back(request);

            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(Protocol { protocols }, ()),
            });
        }

        debug_assert!(self.pending_outbound.is_empty());

        if self.pending_outbound.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_outbound.shrink_to_fit();
        }

        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            }
            _ => {}
        }
    }
}
