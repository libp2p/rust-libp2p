// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::protocol::{RemoteInfo, IdentifyProtocolConfig, ReplySubstream};
use futures::prelude::*;
use libp2p_core::upgrade::{
    InboundUpgrade,
    OutboundUpgrade,
    Negotiated
};
use libp2p_swarm::{
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr
};
use smallvec::SmallVec;
use std::{io, marker::PhantomData, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::{Delay, Instant};

/// Delay between the moment we connect and the first time we identify.
const DELAY_TO_FIRST_ID: Duration = Duration::from_millis(500);
/// After an identification succeeded, wait this long before the next time.
const DELAY_TO_NEXT_ID: Duration = Duration::from_secs(5 * 60);
/// After we failed to identify the remote, try again after the given delay.
const TRY_AGAIN_ON_ERR: Duration = Duration::from_secs(60 * 60);

/// Protocol handler for sending and receiving identification requests.
///
/// Outbound requests are sent periodically. The handler performs expects
/// at least one identification request to be answered by the remote before
/// permitting the underlying connection to be closed.
pub struct IdentifyHandler<TSubstream> {
    /// Configuration for the protocol.
    config: IdentifyProtocolConfig,

    /// Pending events to yield.
    events: SmallVec<[IdentifyHandlerEvent<TSubstream>; 4]>,

    /// Future that fires when we need to identify the node again.
    next_id: Delay,

    /// Whether the handler should keep the connection alive.
    keep_alive: KeepAlive,

    /// Marker for strong typing.
    marker: PhantomData<TSubstream>,
}

/// Event produced by the `IdentifyHandler`.
#[derive(Debug)]
pub enum IdentifyHandlerEvent<TSubstream> {
    /// We obtained identification information from the remote
    Identified(RemoteInfo),
    /// We received a request for identification.
    Identify(ReplySubstream<Negotiated<TSubstream>>),
    /// Failed to identify the remote.
    IdentificationError(ProtocolsHandlerUpgrErr<io::Error>),
}

impl<TSubstream> IdentifyHandler<TSubstream> {
    /// Creates a new `IdentifyHandler`.
    pub fn new() -> Self {
        IdentifyHandler {
            config: IdentifyProtocolConfig,
            events: SmallVec::new(),
            next_id: Delay::new(Instant::now() + DELAY_TO_FIRST_ID),
            keep_alive: KeepAlive::Yes,
            marker: PhantomData,
        }
    }
}

impl<TSubstream> ProtocolsHandler for IdentifyHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = ();
    type OutEvent = IdentifyHandlerEvent<TSubstream>;
    type Error = wasm_timer::Error;
    type Substream = TSubstream;
    type InboundProtocol = IdentifyProtocolConfig;
    type OutboundProtocol = IdentifyProtocolConfig;
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(self.config.clone())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output
    ) {
        self.events.push(IdentifyHandlerEvent::Identify(protocol))
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        _info: Self::OutboundOpenInfo,
    ) {
        self.events.push(IdentifyHandlerEvent::Identified(protocol));
        self.keep_alive = KeepAlive::No;
    }

    fn inject_event(&mut self, _: Self::InEvent) {}

    fn inject_dial_upgrade_error(
        &mut self,
        _info: Self::OutboundOpenInfo,
        err: ProtocolsHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error
        >
    ) {
        self.events.push(IdentifyHandlerEvent::IdentificationError(err));
        self.keep_alive = KeepAlive::No;
        self.next_id.reset(Instant::now() + TRY_AGAIN_ON_ERR);
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(&mut self) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            IdentifyHandlerEvent<TSubstream>,
        >,
        Self::Error,
    > {
        if !self.events.is_empty() {
            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(
                self.events.remove(0),
            )));
        }

        // Poll the future that fires when we need to identify the node again.
        match self.next_id.poll()? {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready(()) => {
                self.next_id.reset(Instant::now() + DELAY_TO_NEXT_ID);
                let ev = ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(self.config.clone()),
                    info: (),
                };
                Ok(Async::Ready(ev))
            }
        }
    }
}
