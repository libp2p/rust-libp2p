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

use crate::protocol::{
    self, Identify, InboundPush, Info, OutboundPush, Protocol, Push, UpgradeError,
};
use either::Either;
use futures::future::BoxFuture;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use futures_timer::Delay;
use libp2p_core::either::EitherOutput;
use libp2p_core::upgrade::SelectUpgrade;
use libp2p_core::{ConnectedPoint, Multiaddr, PeerId, PublicKey};
use libp2p_swarm::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, IntoConnectionHandler,
    KeepAlive, NegotiatedSubstream, SubstreamProtocol,
};
use log::warn;
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::{io, pin::Pin, task::Context, task::Poll, time::Duration};

pub struct Proto {
    initial_delay: Duration,
    interval: Duration,
    public_key: PublicKey,
    protocol_version: String,
    agent_version: String,
}

impl Proto {
    pub fn new(
        initial_delay: Duration,
        interval: Duration,
        public_key: PublicKey,
        protocol_version: String,
        agent_version: String,
    ) -> Self {
        Proto {
            initial_delay,
            interval,
            public_key,
            protocol_version,
            agent_version,
        }
    }
}

impl IntoConnectionHandler for Proto {
    type Handler = Handler;

    fn into_handler(self, remote_peer_id: &PeerId, endpoint: &ConnectedPoint) -> Self::Handler {
        let observed_addr = match endpoint {
            ConnectedPoint::Dialer { address, .. } => address,
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr,
        };

        Handler::new(
            self.initial_delay,
            self.interval,
            *remote_peer_id,
            self.public_key,
            self.protocol_version,
            self.agent_version,
            observed_addr.clone(),
        )
    }

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
        SelectUpgrade::new(Identify, Push::inbound())
    }
}

/// Protocol handler for sending and receiving identification requests.
///
/// Outbound requests are sent periodically. The handler performs expects
/// at least one identification request to be answered by the remote before
/// permitting the underlying connection to be closed.
pub struct Handler {
    remote_peer_id: PeerId,
    inbound_identify_push: Option<BoxFuture<'static, Result<Info, UpgradeError>>>,
    /// Pending events to yield.
    events: SmallVec<
        [ConnectionHandlerEvent<Either<Identify, Push<OutboundPush>>, (), Event, io::Error>; 4],
    >,

    /// Streams awaiting `BehaviourInfo` to then send identify requests.
    reply_streams: VecDeque<NegotiatedSubstream>,

    /// Pending identification replies, awaiting being sent.
    pending_replies: FuturesUnordered<BoxFuture<'static, Result<PeerId, UpgradeError>>>,

    /// Future that fires when we need to identify the node again.
    trigger_next_identify: Delay,

    /// Whether the handler should keep the connection alive.
    keep_alive: KeepAlive,

    /// The interval of `trigger_next_identify`, i.e. the recurrent delay.
    interval: Duration,

    /// The public key of the local peer.
    public_key: PublicKey,

    /// Application-specific version of the protocol family used by the peer,
    /// e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    protocol_version: String,

    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    agent_version: String,

    /// Address observed by or for the remote.
    observed_addr: Multiaddr,
}

/// An event from `Behaviour` with the information requested by the `Handler`.
#[derive(Debug)]
pub struct InEvent {
    /// The addresses that the peer is listening on.
    pub listen_addrs: Vec<Multiaddr>,

    /// The list of protocols supported by the peer, e.g. `/ipfs/ping/1.0.0`.
    pub supported_protocols: Vec<String>,

    /// The protocol w.r.t. the information requested.
    pub protocol: Protocol,
}

/// Event produced by the `Handler`.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Event {
    /// We obtained identification information from the remote.
    Identified(Info),
    /// We replied to an identification request from the remote.
    Identification(PeerId),
    /// We actively pushed our identification information to the remote.
    IdentificationPushed,
    /// We received a request for identification.
    Identify,
    /// Failed to identify the remote, or to reply to an identification request.
    IdentificationError(ConnectionHandlerUpgrErr<UpgradeError>),
}

impl Handler {
    /// Creates a new `Handler`.
    pub fn new(
        initial_delay: Duration,
        interval: Duration,
        remote_peer_id: PeerId,
        public_key: PublicKey,
        protocol_version: String,
        agent_version: String,
        observed_addr: Multiaddr,
    ) -> Self {
        Self {
            remote_peer_id,
            inbound_identify_push: Default::default(),
            events: SmallVec::new(),
            reply_streams: VecDeque::new(),
            pending_replies: FuturesUnordered::new(),
            trigger_next_identify: Delay::new(initial_delay),
            keep_alive: KeepAlive::Yes,
            interval,
            public_key,
            protocol_version,
            agent_version,
            observed_addr,
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
            EitherOutput::First(substream) => {
                self.events
                    .push(ConnectionHandlerEvent::Custom(Event::Identify));
                if !self.reply_streams.is_empty() {
                    warn!(
                        "New inbound identify request from {} while a previous one \
                         is still pending. Queueing the new one.",
                        self.remote_peer_id,
                    );
                }
                self.reply_streams.push_back(substream);
            }
            EitherOutput::Second(fut) => {
                if self.inbound_identify_push.replace(fut).is_some() {
                    warn!(
                        "New inbound identify push stream from {} while still \
                         upgrading previous one. Replacing previous with new.",
                        self.remote_peer_id,
                    );
                }
            }
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: output, ..
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        match output {
            EitherOutput::First(remote_info) => {
                self.events
                    .push(ConnectionHandlerEvent::Custom(Event::Identified(
                        remote_info,
                    )));
                self.keep_alive = KeepAlive::No;
            }
            EitherOutput::Second(()) => self
                .events
                .push(ConnectionHandlerEvent::Custom(Event::IdentificationPushed)),
        }
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error: err, .. }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        use libp2p_core::upgrade::UpgradeError;

        let err = err.map_upgrade_err(|e| match e {
            UpgradeError::Select(e) => UpgradeError::Select(e),
            UpgradeError::Apply(Either::Left(ioe)) => UpgradeError::Apply(ioe),
            UpgradeError::Apply(Either::Right(ioe)) => UpgradeError::Apply(ioe),
        });
        self.events
            .push(ConnectionHandlerEvent::Custom(Event::IdentificationError(
                err,
            )));
        self.keep_alive = KeepAlive::No;
        self.trigger_next_identify.reset(self.interval);
    }
}

impl ConnectionHandler for Handler {
    type InEvent = InEvent;
    type OutEvent = Event;
    type Error = io::Error;
    type InboundProtocol = SelectUpgrade<Identify, Push<InboundPush>>;
    type OutboundProtocol = Either<Identify, Push<OutboundPush>>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(SelectUpgrade::new(Identify, Push::inbound()), ())
    }

    fn on_behaviour_event(
        &mut self,
        InEvent {
            listen_addrs,
            supported_protocols,
            protocol,
        }: Self::InEvent,
    ) {
        let info = Info {
            public_key: self.public_key.clone(),
            protocol_version: self.protocol_version.clone(),
            agent_version: self.agent_version.clone(),
            listen_addrs,
            protocols: supported_protocols,
            observed_addr: self.observed_addr.clone(),
        };

        match protocol {
            Protocol::Push => {
                self.events
                    .push(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(Either::Right(Push::outbound(info)), ()),
                    });
            }
            Protocol::Identify(_) => {
                let substream = self
                    .reply_streams
                    .pop_front()
                    .expect("A BehaviourInfo reply should have a matching substream.");
                let peer = self.remote_peer_id;
                let fut = Box::pin(async move {
                    protocol::send(substream, info).await?;
                    Ok(peer)
                });
                self.pending_replies.push(fut);
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Event, Self::Error>,
    > {
        if !self.events.is_empty() {
            return Poll::Ready(self.events.remove(0));
        }

        // Poll the future that fires when we need to identify the node again.
        match Future::poll(Pin::new(&mut self.trigger_next_identify), cx) {
            Poll::Pending => {}
            Poll::Ready(()) => {
                self.trigger_next_identify.reset(self.interval);
                let ev = ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(Either::Left(Identify), ()),
                };
                return Poll::Ready(ev);
            }
        }

        if let Some(Poll::Ready(res)) = self
            .inbound_identify_push
            .as_mut()
            .map(|f| f.poll_unpin(cx))
        {
            self.inbound_identify_push.take();

            if let Ok(info) = res {
                return Poll::Ready(ConnectionHandlerEvent::Custom(Event::Identified(info)));
            }
        }

        // Check for pending replies to send.
        match self.pending_replies.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok(peer_id))) => Poll::Ready(ConnectionHandlerEvent::Custom(
                Event::Identification(peer_id),
            )),
            Poll::Ready(Some(Err(err))) => Poll::Ready(ConnectionHandlerEvent::Custom(
                Event::IdentificationError(ConnectionHandlerUpgrErr::Upgrade(
                    libp2p_core::upgrade::UpgradeError::Apply(err),
                )),
            )),
            Poll::Ready(None) | Poll::Pending => Poll::Pending,
        }
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
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            ConnectionEvent::AddressChange(_) | ConnectionEvent::ListenUpgradeError(_) => {}
        }
    }
}
