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

use futures::prelude::*;
use libp2p_core::nodes::handled_node::NodeHandlerEndpoint;
use libp2p_core::nodes::protocols_handler::{ProtocolsHandler, ProtocolsHandlerEvent};
use libp2p_core::upgrade::{self, toggleable::Toggleable};
use libp2p_core::{ConnectionUpgrade, Multiaddr};
use std::io;
use std::marker::PhantomData;
use std::time::{Duration, Instant};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Delay;
use void::Void;
use {IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};

/// Delay between the moment we connect and the first time we identify.
const DELAY_TO_FIRST_ID: Duration = Duration::from_millis(500);
/// After an identification succeeded, wait this long before the next time.
const DELAY_TO_NEXT_ID: Duration = Duration::from_secs(5 * 60);
/// After we failed to identify the remote, try again after the given delay.
const TRY_AGAIN_ON_ERR: Duration = Duration::from_secs(60 * 60);

/// Protocol handler that identifies the remote at a regular period.
pub struct PeriodicIdentification<TSubstream> {
    /// Configuration for the protocol.
    config: Toggleable<IdentifyProtocolConfig>,

    /// If `Some`, we successfully generated an `PeriodicIdentificationEvent` and we will produce
    /// it the next time `poll()` is invoked.
    pending_result: Option<PeriodicIdentificationEvent>,

    /// Future that fires when we need to identify the node again. If `None`, means that we should
    /// shut down.
    next_id: Option<Delay>,

    /// Marker for strong typing.
    marker: PhantomData<TSubstream>,
}

/// Event produced by the periodic identifier.
#[derive(Debug)]
pub enum PeriodicIdentificationEvent {
    /// We obtained identification information from the remote
    Identified {
        /// Information of the remote.
        info: IdentifyInfo,
        /// Address the remote observes us as.
        observed_addr: Multiaddr,
    },

    /// Failed to identify the remote.
    IdentificationError(io::Error),
}

impl<TSubstream> PeriodicIdentification<TSubstream> {
    /// Builds a new `PeriodicIdentification`.
    #[inline]
    pub fn new() -> Self {
        PeriodicIdentification {
            config: upgrade::toggleable(IdentifyProtocolConfig),
            pending_result: None,
            next_id: Some(Delay::new(Instant::now() + DELAY_TO_FIRST_ID)),
            marker: PhantomData,
        }
    }
}

impl<TSubstream> ProtocolsHandler for PeriodicIdentification<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static, // TODO: remove useless bounds
{
    type InEvent = Void;
    type OutEvent = PeriodicIdentificationEvent;
    type Substream = TSubstream;
    type Protocol = Toggleable<IdentifyProtocolConfig>;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::Protocol {
        let mut upgrade = self.config.clone();
        upgrade.disable();
        upgrade
    }

    fn inject_fully_negotiated(
        &mut self,
        protocol: <Self::Protocol as ConnectionUpgrade<TSubstream>>::Output,
        _endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        match protocol {
            IdentifyOutput::RemoteInfo {
                info,
                observed_addr,
            } => {
                self.pending_result = Some(PeriodicIdentificationEvent::Identified {
                    info,
                    observed_addr,
                });
            }
            IdentifyOutput::Sender { .. } => unreachable!(
                "Sender can only be produced if we listen for the identify \
                 protocol ; however we disable it in listen_protocol"
            ),
        }
    }

    #[inline]
    fn inject_event(&mut self, _: &Self::InEvent) {}

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, err: io::Error) {
        self.pending_result = Some(PeriodicIdentificationEvent::IdentificationError(err));
        if let Some(ref mut next_id) = self.next_id {
            next_id.reset(Instant::now() + TRY_AGAIN_ON_ERR);
        }
    }

    #[inline]
    fn shutdown(&mut self) {
        self.next_id = None;
    }

    fn poll(
        &mut self,
    ) -> Poll<
        Option<
            ProtocolsHandlerEvent<
                Self::Protocol,
                Self::OutboundOpenInfo,
                PeriodicIdentificationEvent,
            >,
        >,
        io::Error,
    > {
        if let Some(pending_result) = self.pending_result.take() {
            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(
                pending_result,
            ))));
        }

        let next_id = match self.next_id {
            Some(ref mut nid) => nid,
            None => return Ok(Async::Ready(None)),
        };

        // Poll the future that fires when we need to identify the node again.
        match next_id.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(())) => {
                next_id.reset(Instant::now() + DELAY_TO_NEXT_ID);
                let mut upgrade = self.config.clone();
                upgrade.enable();
                let ev = ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info: () };
                Ok(Async::Ready(Some(ev)))
            }
            Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
        }
    }
}
