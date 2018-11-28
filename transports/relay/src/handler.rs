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
use libp2p_core::{ConnectionUpgrade, Multiaddr, PeerId};
use protocol::{RelayDestinationRequest, RelayHopRequest, RelayOutput, RelayConfig};
use std::{io, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};

/// Protocol handler that identifies the remote at a regular period.
pub struct RelayHandler<TSubstream, TDestSubstream> {
    /// Configuration for the protocol.
    config: RelayConfig,

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
    marker: PhantomData<TDestSubstream>
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

/// Event produced by the relay handler.
//#[derive(Debug)]      // TODO: restore
pub enum RelayHandlerIn<TSubstream, TDestSubstream> {
    /// Accept a hop request from the remote.
    AcceptHopRequest {
        /// The request that was produced by the handler earlier.
        request: RelayHandlerHopRequest<TSubstream>,
        /// The substream to the destination.
        dest_substream: TDestSubstream,
    },
    /// Denies a hop request from the remote.
    DenyHopRequest(RelayHandlerHopRequest<TSubstream>),
    /// Opens a new substream to the remote and asks it to relay communications to a third party.
    RelayRequest {
        /// Id of the peer to connect to.
        target: PeerId,
        /// Addresses known for this peer.
        addresses: Vec<Multiaddr>,
    },
}

/// The remote wants us to be treated as a destination.
pub struct RelayHandlerHopRequest<TSubstream> {
    inner: RelayHopRequest<TSubstream>,
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
    inner: RelayDestinationRequest<TSubstream>,
}

impl<TSubstream> RelayHandlerDestRequest<TSubstream>
where TSubstream: AsyncRead + AsyncWrite + 'static
{
    /// Peer id of the source that is being relayed.
    #[inline]
    pub fn source_id(&self) -> &PeerId {
        self.inner.source_id()
    }

    // TODO: addresses

    /// Accepts the request. Produces a `Future` that sends back a success message then provides
    /// the stream to the source.
    #[inline]
    pub fn accept(self) -> impl Future<Item = TSubstream, Error = io::Error> {
        self.inner.accept()
    }

    /// Denies the request. Produces a `Future` that sends back an error message to the proxyy.
    #[inline]
    pub fn deny(self) -> impl Future<Item = (), Error = io::Error> {
        self.inner.deny()
    }
}

impl<TSubstream, TDestSubstream> RelayHandler<TSubstream, TDestSubstream> {
    /// Builds a new `RelayHandler`.
    #[inline]
    pub fn new() -> Self {
        RelayHandler {
            config: RelayConfig::new(),
            shutdown: false,
            active_futures: Vec::new(),
            relay_requests: Vec::new(),
            queued_events: Vec::new(),
            marker: PhantomData,
        }
    }
}

impl<TSubstream, TDestSubstream> ProtocolsHandler for RelayHandler<TSubstream, TDestSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static, // TODO: remove useless bounds
    TDestSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static, // TODO: remove useless bounds
{
    type InEvent = RelayHandlerIn<TSubstream, TDestSubstream>;
    type OutEvent = RelayHandlerEvent<TSubstream>;
    type Substream = TSubstream;
    type Protocol = RelayConfig;
    // The information is the peer we want to relay communications to.
    type OutboundOpenInfo = (PeerId, Vec<Multiaddr>);

    #[inline]
    fn listen_protocol(&self) -> Self::Protocol {
        self.config.clone()
    }

    fn inject_fully_negotiated(
        &mut self,
        protocol: <Self::Protocol as ConnectionUpgrade<TSubstream>>::Output,
        endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        match (protocol, endpoint) {
            (RelayOutput::HopRequest(inner), _) => {
                // The remote wants us to relay communications to a third party.
                let ev = RelayHandlerEvent::HopRequest(RelayHandlerHopRequest {
                    inner,
                });
                self.queued_events.push(ev);
            },
            (RelayOutput::DestinationRequest(inner), _) => {
                // The remote wants to use us as a destination.
                let ev = RelayHandlerEvent::DestinationRequest(RelayHandlerDestRequest {
                    inner,
                });
                self.queued_events.push(ev);
            },
            (RelayOutput::ProxyRequest(request), NodeHandlerEndpoint::Dialer((peer_id, addresses))) => {
                // We can ask the remote for what we want it to do.
                request.request(peer_id, addresses);
            },
            (RelayOutput::ProxyRequest(_), NodeHandlerEndpoint::Listener) => {
                // This shouldn't happen.
                debug_assert!(false, "the protocol forbids ProxyRequest when listening");
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
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: io::Error) {
        // TODO: report?
    }

    #[inline]
    fn shutdown(&mut self) {
        self.shutdown = true;
    }

    fn poll(
        &mut self,
    ) -> Poll<
        Option<
            ProtocolsHandlerEvent<
                Self::Protocol,
                Self::OutboundOpenInfo,
                RelayHandlerEvent<TSubstream>,
            >,
        >,
        io::Error,
    > {
        // Open substreams if necessary.
        if !self.relay_requests.is_empty() {
            let rq = self.relay_requests.remove(0);
            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                info: rq,
                upgrade: self.config.clone(),
            })));
        }

        // Report the queued events.
        if !self.queued_events.is_empty() {
            let event = self.queued_events.remove(0);
            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(event))));
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
            return Ok(Async::Ready(None));
        }

        Ok(Async::NotReady)
    }
}
