use futures::future;
use libp2p_core::{
    upgrade::NegotiationError, InboundUpgrade, OutboundUpgrade, UpgradeError, UpgradeInfo,
};
use libp2p_swarm::{
    protocols_handler::InboundUpgradeSend, KeepAlive, NegotiatedSubstream, ProtocolsHandler,
    ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use smallvec::SmallVec;
use std::{
    collections::VecDeque,
    convert::Infallible,
    marker::PhantomData,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::Poll,
    time::Instant,
};

use crate::{
    InboundStreamId, OutboundStreamId, StreamHandle, StreamingCodec, StreamingConfig,
    EMPTY_QUEUE_SHRINK_THRESHOLD,
};

pub(crate) type RefCount = Arc<PhantomData<u8>>;
#[derive(Debug)]
pub enum StreamId {
    Inbound(InboundStreamId),
    Outbound(OutboundStreamId),
}
pub struct StreamingProtocolsHandler<T: StreamingCodec> {
    config: StreamingConfig,
    /// The current connection keep-alive.
    keep_alive: KeepAlive,
    /// Queue of events to emit in `poll()`.
    pending_events: VecDeque<StreamingProtocolsHandlerEvent<T>>,
    /// Outbound upgrades waiting to be emitted as an `OutboundSubstreamRequest`.
    outbound: VecDeque<OutboundStreamId>,
    /// Live streams.
    open_streams: SmallVec<[(StreamId, RefCount); 2]>,
    /// Counter for inbound stream ids.
    inbound_stream_id: Arc<AtomicU64>,
    /// A pending fatal error that results in the connection being closed.
    pending_error: Option<ProtocolsHandlerUpgrErr<std::convert::Infallible>>,
    _codec: PhantomData<T>,
}

impl<T: StreamingCodec> StreamingProtocolsHandler<T> {
    pub(crate) fn new(inbound_stream_id: Arc<AtomicU64>, config: StreamingConfig) -> Self {
        Self {
            config,
            keep_alive: KeepAlive::Yes,
            pending_events: VecDeque::default(),
            outbound: VecDeque::default(),
            inbound_stream_id,
            open_streams: Default::default(),
            pending_error: None,
            _codec: Default::default(),
        }
    }
}

#[derive(Debug)]
pub enum StreamingProtocolsHandlerEvent<T: StreamingCodec> {
    NewIncoming {
        id: InboundStreamId,
        stream: StreamHandle<T::Upgrade>,
    },
    StreamOpened {
        id: OutboundStreamId,
        stream: StreamHandle<T::Upgrade>,
    },
    InboundStreamClosed {
        id: InboundStreamId,
    },
    OutboundStreamClosed {
        id: OutboundStreamId,
    },
    OutboundTimeout(OutboundStreamId),
    OutboundUnsupportedProtocols(OutboundStreamId),
    InboundTimeout(InboundStreamId),
    InboundUnsupportedProtocols(InboundStreamId),
}

#[derive(Debug, Default)]
pub struct StreamingProtocol<T: StreamingCodec> {
    _codec: PhantomData<T>,
}
impl<T: StreamingCodec> StreamingProtocol<T> {
    fn new() -> Self {
        Self {
            _codec: Default::default(),
        }
    }
}

impl<T: StreamingCodec> UpgradeInfo for StreamingProtocol<T> {
    type Info = T::Protocol;

    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        // must start with '/'
        std::iter::once(T::protocol_name())
    }
}

impl<T: StreamingCodec> InboundUpgrade<NegotiatedSubstream> for StreamingProtocol<T> {
    type Output = T::Upgrade;

    type Error = std::convert::Infallible;

    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(T::upgrade(io))
    }
}

impl<T: StreamingCodec> OutboundUpgrade<NegotiatedSubstream> for StreamingProtocol<T> {
    type Output = T::Upgrade;

    type Error = std::convert::Infallible;

    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(T::upgrade(io))
    }
}

impl<T: StreamingCodec + Send + 'static> ProtocolsHandler for StreamingProtocolsHandler<T> {
    type InEvent = OutboundStreamId;

    type OutEvent = StreamingProtocolsHandlerEvent<T>;

    type Error = ProtocolsHandlerUpgrErr<Infallible>;

    type InboundProtocol = StreamingProtocol<T>;

    type OutboundProtocol = StreamingProtocol<T>;

    type InboundOpenInfo = InboundStreamId;

    type OutboundOpenInfo = OutboundStreamId;

    fn listen_protocol(
        &self,
    ) -> libp2p_swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        let stream_id = InboundStreamId(self.inbound_stream_id.fetch_add(1, Ordering::Relaxed));
        tracing::trace!("new listen_protocol with stream_id {:?}", stream_id);
        SubstreamProtocol::new(StreamingProtocol::new(), stream_id)
    }

    fn inject_fully_negotiated_inbound(&mut self, handle: T::Upgrade, info: Self::InboundOpenInfo) {
        tracing::trace!("New Inbound stream {:?}", info);
        self.keep_alive = KeepAlive::Yes;
        let marker: RefCount = Default::default();
        let marker_c = marker.clone();
        self.open_streams.push((StreamId::Inbound(info), marker));
        let ev = StreamingProtocolsHandlerEvent::NewIncoming {
            id: info,
            stream: StreamHandle::new(handle, marker_c),
        };
        self.pending_events.push_back(ev);
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        handle: T::Upgrade,
        info: Self::OutboundOpenInfo,
    ) {
        tracing::trace!("New outbound stream {:?}", info);

        self.keep_alive = KeepAlive::Yes;
        let marker: RefCount = Default::default();
        let marker_c = marker.clone();
        self.open_streams.push((StreamId::Outbound(info), marker));
        let ev = StreamingProtocolsHandlerEvent::StreamOpened {
            id: info,
            stream: StreamHandle::new(handle, marker_c),
        };
        self.pending_events.push_back(ev);
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        tracing::trace!("inject_event {:?}", event);
        self.keep_alive = KeepAlive::Yes;
        self.outbound.push_back(event);
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        err: ProtocolsHandlerUpgrErr<std::convert::Infallible>,
    ) {
        match err {
            ProtocolsHandlerUpgrErr::Timeout => {
                self.pending_events
                    .push_back(StreamingProtocolsHandlerEvent::OutboundTimeout(info));
            }
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(NegotiationError::Failed)) => {
                self.pending_events
                    .push_back(StreamingProtocolsHandlerEvent::OutboundUnsupportedProtocols(info))
            }
            _ => {
                self.pending_error = Some(err);
            }
        }
    }

    fn connection_keep_alive(&self) -> libp2p_swarm::KeepAlive {
        self.keep_alive
    }

    fn inject_listen_upgrade_error(
        &mut self,
        info: Self::InboundOpenInfo,
        err: ProtocolsHandlerUpgrErr<<Self::InboundProtocol as InboundUpgradeSend>::Error>,
    ) {
        match err {
            ProtocolsHandlerUpgrErr::Timeout => {
                self.pending_events
                    .push_back(StreamingProtocolsHandlerEvent::InboundTimeout(info));
            }
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(NegotiationError::Failed)) => {
                self.pending_events.push_back(
                    StreamingProtocolsHandlerEvent::InboundUnsupportedProtocols(info),
                )
            }
            _ => {
                self.pending_error = Some(err);
            }
        }
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<
        libp2p_swarm::ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(err) = self.pending_error.take() {
            return Poll::Ready(ProtocolsHandlerEvent::Close(err));
        }

        {
            // Check open streams
            let open_streams = &mut self.open_streams;
            let pending_events = &mut self.pending_events;
            open_streams.retain(|(id, marker)| {
                if Arc::strong_count(marker) == 1 {
                    tracing::debug!("Stream {:?} was dropped", id);
                    let ev = match *id {
                        StreamId::Inbound(id) => {
                            StreamingProtocolsHandlerEvent::InboundStreamClosed { id }
                        }
                        StreamId::Outbound(id) => {
                            StreamingProtocolsHandlerEvent::OutboundStreamClosed { id }
                        }
                    };
                    pending_events.push_back(ev);
                    false
                } else {
                    true
                }
            });
        }

        // Drain pending events.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ProtocolsHandlerEvent::Custom(event));
        } else if self.pending_events.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.pending_events.shrink_to_fit();
        }
        // Emit outbound requests.
        if let Some(info) = self.outbound.pop_front() {
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(StreamingProtocol::new(), info)
                    .with_timeout(self.config.substream_timeout),
            });
        }

        debug_assert!(self.outbound.is_empty());

        if self.outbound.capacity() > EMPTY_QUEUE_SHRINK_THRESHOLD {
            self.outbound.shrink_to_fit();
        }

        if self.keep_alive.is_yes() && self.open_streams.is_empty() {
            // No open streams but there may be outbound upgrades.
            let until =
                Instant::now() + self.config.substream_timeout + self.config.keep_alive_timeout;
            self.keep_alive = KeepAlive::Until(until);
        }

        Poll::Pending
    }
}
