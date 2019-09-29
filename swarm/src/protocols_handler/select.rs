// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::protocols_handler::{
    KeepAlive,
    SubstreamProtocol,
    IntoProtocolsHandler,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr,
};
use futures::prelude::*;
use libp2p_core::{
    ConnectedPoint,
    PeerId,
    either::{EitherError, EitherOutput},
    upgrade::{InboundUpgrade, OutboundUpgrade, EitherUpgrade, SelectUpgrade, UpgradeError}
};
use std::cmp;
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `IntoProtocolsHandler` that combines two protocols into one.
#[derive(Debug, Clone)]
pub struct IntoProtocolsHandlerSelect<TProto1, TProto2> {
    /// The first protocol.
    proto1: TProto1,
    /// The second protocol.
    proto2: TProto2,
}

impl<TProto1, TProto2> IntoProtocolsHandlerSelect<TProto1, TProto2> {
    /// Builds a `IntoProtocolsHandlerSelect`.
    #[inline]
    pub(crate) fn new(proto1: TProto1, proto2: TProto2) -> Self {
        IntoProtocolsHandlerSelect {
            proto1,
            proto2,
        }
    }
}

impl<TProto1, TProto2, TSubstream> IntoProtocolsHandler for IntoProtocolsHandlerSelect<TProto1, TProto2>
where
    TProto1: IntoProtocolsHandler,
    TProto2: IntoProtocolsHandler,
    TProto1::Handler: ProtocolsHandler<Substream = TSubstream>,
    TProto2::Handler: ProtocolsHandler<Substream = TSubstream>,
    TSubstream: AsyncRead + AsyncWrite,
    <TProto1::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<TSubstream>,
    <TProto2::Handler as ProtocolsHandler>::InboundProtocol: InboundUpgrade<TSubstream>,
    <TProto1::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<TSubstream>,
    <TProto2::Handler as ProtocolsHandler>::OutboundProtocol: OutboundUpgrade<TSubstream>
{
    type Handler = ProtocolsHandlerSelect<TProto1::Handler, TProto2::Handler>;

    fn into_handler(self, remote_peer_id: &PeerId, connected_point: &ConnectedPoint) -> Self::Handler {
        ProtocolsHandlerSelect {
            proto1: self.proto1.into_handler(remote_peer_id, connected_point),
            proto2: self.proto2.into_handler(remote_peer_id, connected_point),
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        SelectUpgrade::new(self.proto1.inbound_protocol(), self.proto2.inbound_protocol())
    }
}

/// Implementation of `ProtocolsHandler` that combines two protocols into one.
#[derive(Debug, Clone)]
pub struct ProtocolsHandlerSelect<TProto1, TProto2> {
    /// The first protocol.
    proto1: TProto1,
    /// The second protocol.
    proto2: TProto2,
}

impl<TProto1, TProto2> ProtocolsHandlerSelect<TProto1, TProto2> {
    /// Builds a `ProtocolsHandlerSelect`.
    #[inline]
    pub(crate) fn new(proto1: TProto1, proto2: TProto2) -> Self {
        ProtocolsHandlerSelect {
            proto1,
            proto2,
        }
    }
}

impl<TSubstream, TProto1, TProto2>
    ProtocolsHandler for ProtocolsHandlerSelect<TProto1, TProto2>
where
    TProto1: ProtocolsHandler<Substream = TSubstream>,
    TProto2: ProtocolsHandler<Substream = TSubstream>,
    TSubstream: AsyncRead + AsyncWrite,
    TProto1::InboundProtocol: InboundUpgrade<TSubstream>,
    TProto2::InboundProtocol: InboundUpgrade<TSubstream>,
    TProto1::OutboundProtocol: OutboundUpgrade<TSubstream>,
    TProto2::OutboundProtocol: OutboundUpgrade<TSubstream>
{
    type InEvent = EitherOutput<TProto1::InEvent, TProto2::InEvent>;
    type OutEvent = EitherOutput<TProto1::OutEvent, TProto2::OutEvent>;
    type Error = EitherError<TProto1::Error, TProto2::Error>;
    type Substream = TSubstream;
    type InboundProtocol = SelectUpgrade<<TProto1 as ProtocolsHandler>::InboundProtocol, <TProto2 as ProtocolsHandler>::InboundProtocol>;
    type OutboundProtocol = EitherUpgrade<TProto1::OutboundProtocol, TProto2::OutboundProtocol>;
    type OutboundOpenInfo = EitherOutput<TProto1::OutboundOpenInfo, TProto2::OutboundOpenInfo>;

    #[inline]
    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        let proto1 = self.proto1.listen_protocol();
        let proto2 = self.proto2.listen_protocol();
        let timeout = std::cmp::max(proto1.timeout(), proto2.timeout()).clone();
        let choice = SelectUpgrade::new(proto1.into_upgrade().1, proto2.into_upgrade().1);
        SubstreamProtocol::new(choice).with_timeout(timeout)
    }

    fn inject_fully_negotiated_outbound(&mut self, protocol: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output, endpoint: Self::OutboundOpenInfo) {
        match (protocol, endpoint) {
            (EitherOutput::First(protocol), EitherOutput::First(info)) =>
                self.proto1.inject_fully_negotiated_outbound(protocol, info),
            (EitherOutput::Second(protocol), EitherOutput::Second(info)) =>
                self.proto2.inject_fully_negotiated_outbound(protocol, info),
            (EitherOutput::First(_), EitherOutput::Second(_)) =>
                panic!("wrong API usage: the protocol doesn't match the upgrade info"),
            (EitherOutput::Second(_), EitherOutput::First(_)) =>
                panic!("wrong API usage: the protocol doesn't match the upgrade info")
        }
    }

    fn inject_fully_negotiated_inbound(&mut self, protocol: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output) {
        match protocol {
            EitherOutput::First(protocol) =>
                self.proto1.inject_fully_negotiated_inbound(protocol),
            EitherOutput::Second(protocol) =>
                self.proto2.inject_fully_negotiated_inbound(protocol)
        }
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            EitherOutput::First(event) => self.proto1.inject_event(event),
            EitherOutput::Second(event) => self.proto2.inject_event(event),
        }
    }

    #[inline]
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {
        match (info, error) {
            (EitherOutput::First(info), ProtocolsHandlerUpgrErr::Timer) => {
                self.proto1.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer)
            },
            (EitherOutput::First(info), ProtocolsHandlerUpgrErr::Timeout) => {
                self.proto1.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout)
            },
            (EitherOutput::First(info), ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(err))) => {
                self.proto1.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(err)))
            },
            (EitherOutput::First(info), ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(err)))) => {
                self.proto1.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(err)))
            },
            (EitherOutput::First(_), ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(_)))) => {
                panic!("Wrong API usage; the upgrade error doesn't match the outbound open info");
            },
            (EitherOutput::Second(info), ProtocolsHandlerUpgrErr::Timeout) => {
                self.proto2.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout)
            },
            (EitherOutput::Second(info), ProtocolsHandlerUpgrErr::Timer) => {
                self.proto2.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer)
            },
            (EitherOutput::Second(info), ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(err))) => {
                self.proto2.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(err)))
            },
            (EitherOutput::Second(info), ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(err)))) => {
                self.proto2.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(err)))
            },
            (EitherOutput::Second(_), ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(_)))) => {
                panic!("Wrong API usage; the upgrade error doesn't match the outbound open info");
            },
        }
    }

    #[inline]
    fn connection_keep_alive(&self) -> KeepAlive {
        cmp::max(self.proto1.connection_keep_alive(), self.proto2.connection_keep_alive())
    }

    fn poll(&mut self) -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>, Self::Error> {

        match self.proto1.poll().map_err(EitherError::A)? {
            Async::Ready(ProtocolsHandlerEvent::Custom(event)) => {
                return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(EitherOutput::First(event))));
            },
            Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol,
                info,
            }) => {
                return Ok(Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol.map_upgrade(EitherUpgrade::A),
                    info: EitherOutput::First(info),
                }));
            },
            Async::NotReady => ()
        };

        match self.proto2.poll().map_err(EitherError::B)? {
            Async::Ready(ProtocolsHandlerEvent::Custom(event)) => {
                return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(EitherOutput::Second(event))));
            },
            Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol,
                info,
            }) => {
                return Ok(Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol.map_upgrade(EitherUpgrade::B),
                    info: EitherOutput::Second(info),
                }));
            },
            Async::NotReady => ()
        };

        Ok(Async::NotReady)
    }
}
