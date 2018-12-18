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

use crate::{
    either::EitherError,
    either::EitherOutput,
    protocols_handler::{ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr},
    upgrade::{
        InboundUpgrade,
        OutboundUpgrade,
        EitherUpgrade,
        SelectUpgrade,
        UpgradeError,
    }
};
use futures::prelude::*;
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `ProtocolsHandler` that combines two protocols into one.
#[derive(Debug, Clone)]
pub struct ProtocolsHandlerSelect<TProto1, TProto2> {
    proto1: TProto1,
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
    type InboundProtocol = SelectUpgrade<TProto1::InboundProtocol, TProto2::InboundProtocol>;
    type OutboundProtocol = EitherUpgrade<TProto1::OutboundProtocol, TProto2::OutboundProtocol>;
    type OutboundOpenInfo = EitherOutput<TProto1::OutboundOpenInfo, TProto2::OutboundOpenInfo>;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        let proto1 = self.proto1.listen_protocol();
        let proto2 = self.proto2.listen_protocol();
        SelectUpgrade::new(proto1, proto2)
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
    fn inject_inbound_closed(&mut self) {
        self.proto1.inject_inbound_closed();
        self.proto2.inject_inbound_closed();
    }

    #[inline]
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {
        match (info, error) {
            (EitherOutput::First(info), ProtocolsHandlerUpgrErr::MuxerDeniedSubstream) => {
                self.proto1.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::MuxerDeniedSubstream)
            },
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
            (EitherOutput::Second(info), ProtocolsHandlerUpgrErr::MuxerDeniedSubstream) => {
                self.proto2.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::MuxerDeniedSubstream)
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
    fn shutdown(&mut self) {
        self.proto1.shutdown();
        self.proto2.shutdown();
    }

    fn poll(&mut self) -> Poll<Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>, Self::Error> {
        match self.proto1.poll().map_err(EitherError::A)? {
            Async::Ready(Some(ProtocolsHandlerEvent::Custom(event))) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(EitherOutput::First(event)))));
            },
            Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info})) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade: EitherUpgrade::A(upgrade),
                    info: EitherOutput::First(info),
                })));
            },
            Async::Ready(None) => return Ok(Async::Ready(None)),
            Async::NotReady => ()
        };

        match self.proto2.poll().map_err(EitherError::B)? {
            Async::Ready(Some(ProtocolsHandlerEvent::Custom(event))) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(EitherOutput::Second(event)))));
            },
            Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info })) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade: EitherUpgrade::B(upgrade),
                    info: EitherOutput::Second(info),
                })));
            },
            Async::Ready(None) => return Ok(Async::Ready(None)),
            Async::NotReady => ()
        };

        Ok(Async::NotReady)
    }
}
