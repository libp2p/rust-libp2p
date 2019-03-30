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

use crate::protocol::{RemoteInfo, IdentifyProtocolConfig};
use futures::prelude::*;
use libp2p_core::{
    protocols_handler::{KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr},
    upgrade::{DeniedUpgrade, OutboundUpgrade}
};
use std::{io, marker::PhantomData, time::{Duration, Instant}};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::{self, Delay};
use void::{Void, unreachable};

/// Delay between the moment we connect and the first time we identify.
const DELAY_TO_FIRST_ID: Duration = Duration::from_millis(500);
/// After an identification succeeded, wait this long before the next time.
const DELAY_TO_NEXT_ID: Duration = Duration::from_secs(5 * 60);
/// After we failed to identify the remote, try again after the given delay.
const TRY_AGAIN_ON_ERR: Duration = Duration::from_secs(60 * 60);

/// Protocol handler that identifies the remote at a regular period.
pub struct PeriodicIdHandler<TSubstream> {
    /// Configuration for the protocol.
    config: IdentifyProtocolConfig,

    /// If `Some`, we successfully generated an `PeriodicIdHandlerEvent` and we will produce
    /// it the next time `poll()` is invoked.
    pending_result: Option<PeriodicIdHandlerEvent>,

    /// Future that fires when we need to identify the node again.
    next_id: Delay,

    /// If `true`, we have started an identification of the remote at least once in the past.
    first_id_happened: bool,

    /// Marker for strong typing.
    marker: PhantomData<TSubstream>,
}

/// Event produced by the periodic identifier.
#[derive(Debug)]
pub enum PeriodicIdHandlerEvent {
    /// We obtained identification information from the remote
    Identified(RemoteInfo),
    /// Failed to identify the remote.
    IdentificationError(ProtocolsHandlerUpgrErr<io::Error>),
}

impl<TSubstream> PeriodicIdHandler<TSubstream> {
    /// Builds a new `PeriodicIdHandler`.
    #[inline]
    pub fn new() -> Self {
        PeriodicIdHandler {
            config: IdentifyProtocolConfig,
            pending_result: None,
            next_id: Delay::new(Instant::now() + DELAY_TO_FIRST_ID),
            first_id_happened: false,
            marker: PhantomData,
        }
    }
}

impl<TSubstream> ProtocolsHandler for PeriodicIdHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = Void;
    type OutEvent = PeriodicIdHandlerEvent;
    type Error = tokio_timer::Error;
    type Substream = TSubstream;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = IdentifyProtocolConfig;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        DeniedUpgrade
    }

    fn inject_fully_negotiated_inbound(&mut self, protocol: Void) {
        unreachable(protocol)
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        _info: Self::OutboundOpenInfo,
    ) {
        self.pending_result = Some(PeriodicIdHandlerEvent::Identified(protocol));
        self.first_id_happened = true;
    }

    #[inline]
    fn inject_event(&mut self, _: Self::InEvent) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, err: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {
        self.pending_result = Some(PeriodicIdHandlerEvent::IdentificationError(err));
        self.first_id_happened = true;
        self.next_id.reset(Instant::now() + TRY_AGAIN_ON_ERR);
    }

    #[inline]
    fn connection_keep_alive(&self) -> KeepAlive {
        if self.first_id_happened {
            KeepAlive::Now
        } else {
            KeepAlive::Forever
        }
    }

    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            PeriodicIdHandlerEvent,
        >,
        Self::Error,
    > {
        if let Some(pending_result) = self.pending_result.take() {
            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(
                pending_result,
            )));
        }

        // Poll the future that fires when we need to identify the node again.
        match self.next_id.poll()? {
            Async::NotReady => Ok(Async::NotReady),
            Async::Ready(()) => {
                self.next_id.reset(Instant::now() + DELAY_TO_NEXT_ID);
                let upgrade = self.config.clone();
                let ev = ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info: () };
                Ok(Async::Ready(ev))
            }
        }
    }
}
