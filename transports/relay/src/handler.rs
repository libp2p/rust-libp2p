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
use libp2p_core::protocols_handler::{ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr};
use libp2p_core::{Multiaddr, PeerId, either, upgrade, upgrade::OutboundUpgrade};
use protocol;
use std::{error, io, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};

/// Protocol handler that identifies the remote at a regular period.
pub struct RelayHandler<TSubstream, TDestSubstream, TSrcSubstream> {
    /// True if wanting to shut down.
    shutdown: bool,

    /// List of futures that must be processed. Contains notably the futures that relay data to
    /// a destination, or the futures that send a negative response back.
    active_futures: Vec<Box<Future<Item = (), Error = io::Error> + Send>>,

    /// List of peers we should ask the remote to relay to.
    relay_requests: Vec<(PeerId, Vec<Multiaddr>)>,

    /// Queue of events to return when polled.
    queued_events: Vec<RelayHandlerEvent<TSubstream>>,

    /// Phantom data.
    marker: PhantomData<(TDestSubstream, TSrcSubstream)>
}

/// Event produced by the relay handler.
//#[derive(Debug)]      // TODO: restore
pub enum RelayHandlerEvent<TSubstream> {
    /// The remote wants us to relay communications to a third party.
    HopRequest(RelayHandlerHopRequest<TSubstream>),

    /// The remote is a relay and is relaying a connection to us. In other words, we are used as
    /// destination.
    DestinationRequest(RelayHandlerDestRequest<TSubstream>),
}

/// Event that can be sent to the relay handler.
//#[derive(Debug)]      // TODO: restore
pub enum RelayHandlerIn<TSubstream, TDestSubstream, TSrcSubstream> {
    /// Accept a hop request from the remote.
    AcceptHopRequest {
        /// The request that was produced by the handler earlier.
        request: RelayHandlerHopRequest<TSubstream>,
        /// The substream to the destination.
        dest_substream: TDestSubstream,
    },

    /// Denies a hop request from the remote.
    DenyHopRequest(RelayHandlerHopRequest<TSubstream>),

    /// Denies a destination request from the remote.
    DenyDestinationRequest(RelayHandlerDestRequest<TSubstream>),

    /// Opens a new substream to the remote and asks it to relay communications to a third party.
    RelayRequest {
        /// Id of the peer to connect to.
        target: PeerId,
        /// Addresses known for this peer.
        addresses: Vec<Multiaddr>,
    },

    /// Asks the node to be used as a destination for a relayed connection.
    DestinationRequest {
        /// Peer id of the node whose communications are being relayed.
        source: PeerId,
        /// Addresses of the node whose communications are being relayed.
        source_addresses: Vec<Multiaddr>,
        /// Substream to the source.
        substream: TSrcSubstream,
    },
}

/// The remote wants us to be treated as a destination.
pub struct RelayHandlerHopRequest<TSubstream> {
    inner: protocol::RelayHopRequest<TSubstream>,
}

impl<TSubstream> RelayHandlerHopRequest<TSubstream>
where TSubstream: AsyncRead + AsyncWrite + 'static
{
    /// Peer id of the target that we should relay to.
    #[inline]
    pub fn target_id(&self) -> &PeerId {
        self.inner.target_id()
    }

    // TODO: addresses
}

/// The remote wants us to be treated as a destination.
pub struct RelayHandlerDestRequest<TSubstream> {
    inner: protocol::RelayDestinationRequest<TSubstream>,
}

impl<TSubstream> RelayHandlerDestRequest<TSubstream>
where TSubstream: AsyncRead + AsyncWrite + 'static
{
    /// Peer id of the source that is being relayed.
    #[inline]
    pub fn source_id(&self) -> &PeerId {
        self.inner.source_id()
    }

    /// Addresses of the source that is being relayed.
    #[inline]
    pub fn source_addresses(&self) -> &PeerId {
        self.inner.source_addresses()
    }

    /// Accepts the request. Produces a `Future` that sends back a success message then provides
    /// the stream to the source.
    #[inline]
    pub fn accept(self) -> impl Future<Item = TSubstream, Error = Box<dyn error::Error + 'static>> {
        self.inner.accept()
    }
}

impl<TSubstream, TDestSubstream, TSrcSubstream> RelayHandler<TSubstream, TDestSubstream, TSrcSubstream> {
    /// Builds a new `RelayHandler`.
    #[inline]
    pub fn new() -> Self {
        RelayHandler {
            shutdown: false,
            active_futures: Vec::new(),
            relay_requests: Vec::new(),
            queued_events: Vec::new(),
            marker: PhantomData,
        }
    }
}

impl<TSubstream, TDestSubstream, TSrcSubstream> ProtocolsHandler for RelayHandler<TSubstream, TDestSubstream, TSrcSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static, // TODO: remove useless bounds
    TDestSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static, // TODO: remove useless bounds
{
    type InEvent = RelayHandlerIn<TSubstream, TDestSubstream, TSrcSubstream>;
    type OutEvent = RelayHandlerEvent<TSubstream>;
    type Error = io::Error;
    type Substream = TSubstream;
    type InboundProtocol = protocol::RelayListen;
    type OutboundProtocol = upgrade::EitherUpgrade<protocol::RelayProxyRequest, protocol::RelayTargetOpen>;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        protocol::RelayListen::new()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as upgrade::InboundUpgrade<TSubstream>>::Output,
    ) {
        match protocol {
            // We have been asked to become a destination.
            protocol::RelayRemoteRequest::DestinationRequest(dest_request) => {

            },
            // We have been asked to act as a proxy.
            protocol::RelayRemoteRequest::HopRequest(hop_request) => {

            },
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as upgrade::OutboundUpgrade<TSubstream>>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        match protocol {
            either::EitherOutput::First(proxy_request) => {

            },
            either::EitherOutput::Second(target_request) => {

            },
        }
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            RelayHandlerIn::AcceptHopRequest { request, dest_substream } => {
                let fut = request.inner.fulfill(dest_substream);
                self.active_futures.push(Box::new(fut));
            },
            RelayHandlerIn::DenyHopRequest(rq) => {
                let fut = rq.inner.deny();
                self.active_futures.push(Box::new(fut));
            },
            RelayHandlerIn::RelayRequest { target, addresses } => {
                self.relay_requests.push((target.clone(), addresses.clone()));
            },
        }
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {
        // TODO: report?
    }

    #[inline]
    fn connection_keep_alive(&self) -> bool {
        true // TODO:
    }

    #[inline]
    fn shutdown(&mut self) {
        self.shutdown = true;
    }

    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            RelayHandlerEvent<TSubstream>,
        >,
        io::Error,
    > {
        // Open substreams if necessary.
        if !self.relay_requests.is_empty() {
            let rq = self.relay_requests.remove(0);
            return Ok(Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                info: rq,
                upgrade: self.config.clone(),
            }));
        }

        // Report the queued events.
        if !self.queued_events.is_empty() {
            let event = self.queued_events.remove(0);
            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(event)));
        }

        // We remove each element from `active_futures` one by one and add them back.
        for n in (0..self.active_futures.len()).rev() {
            let mut future = self.active_futures.swap_remove(n);
            match future.poll() {
                // Don't add back the future if it's finished or errors.
                Ok(Async::Ready(())) | Err(_) => {},
                Ok(Async::NotReady) => self.active_futures.push(future),
            }
        }

        // Shut down process.
        if self.shutdown {
            self.active_futures.clear();     // TODO: not a proper shutdown
            return Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown));
        }

        Ok(Async::NotReady)
    }
}
