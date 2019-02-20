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

use crate::{
    either::EitherOutput,
    protocols_handler::{KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr},
    upgrade::{
        DeniedUpgrade,
        EitherUpgrade,
        InboundUpgrade,
        OutboundUpgrade,
    }
};
use futures::prelude::*;
use std::mem;

/// Wrapper around a protocol handler and ignores all further method calls once it has shut down.
#[derive(Debug, Copy, Clone)]
pub struct Fuse<TProtoHandler> {
    inner: State<TProtoHandler>,
}

#[derive(Debug, Copy, Clone)]
enum State<TProtoHandler> {
    Normal(TProtoHandler),
    ShuttingDown(TProtoHandler),
    Shutdown,
}

impl<TProtoHandler> State<TProtoHandler> {
    fn as_ref(&self) -> Option<&TProtoHandler> {
        match self {
            State::Normal(h) => Some(h),
            State::ShuttingDown(h) => Some(h),
            State::Shutdown => None,
        }
    }

    fn as_mut(&mut self) -> Option<&mut TProtoHandler> {
        match self {
            State::Normal(h) => Some(h),
            State::ShuttingDown(h) => Some(h),
            State::Shutdown => None,
        }
    }
}

impl<TProtoHandler> Fuse<TProtoHandler> {
    /// Creates a `Fuse`.
    #[inline]
    pub(crate) fn new(inner: TProtoHandler) -> Self {
        Fuse {
            inner: State::Normal(inner),
        }
    }

    /// Returns true if `shutdown()` has been called in the past, or if polling has returned
    /// `Shutdown` in the past.
    pub fn is_shutting_down_or_shutdown(&self) -> bool {
        match self.inner {
            State::Normal(_) => false,
            State::ShuttingDown(_) => true,
            State::Shutdown => true,
        }
    }

    /// Returns true if polling has returned `Shutdown` in the past.
    #[inline]
    pub fn is_shutdown(&self) -> bool {
        if let State::Shutdown = self.inner {
            true
        } else {
            false
        }
    }
}

impl<TProtoHandler> ProtocolsHandler for Fuse<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler,
{
    type InEvent = TProtoHandler::InEvent;
    type OutEvent = TProtoHandler::OutEvent;
    type Error = TProtoHandler::Error;
    type Substream = TProtoHandler::Substream;
    type InboundProtocol = EitherUpgrade<TProtoHandler::InboundProtocol, DeniedUpgrade>;
    type OutboundProtocol = TProtoHandler::OutboundProtocol;
    type OutboundOpenInfo = TProtoHandler::OutboundOpenInfo;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        if let Some(inner) = self.inner.as_ref() {
            EitherUpgrade::A(inner.listen_protocol())
        } else {
            EitherUpgrade::B(DeniedUpgrade)
        }
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output
    ) {
        match (protocol, self.inner.as_mut()) {
            (EitherOutput::First(proto), Some(inner)) => {
                inner.inject_fully_negotiated_inbound(proto)
            },
            (EitherOutput::Second(_), None) => {}
            (EitherOutput::First(_), None) => {} // Can happen if we shut down during an upgrade.
            (EitherOutput::Second(_), Some(_)) => {
                panic!("Wrong API usage; an upgrade was passed to a different object that the \
                    one that asked for the upgrade")
            },
        }
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        info: Self::OutboundOpenInfo
    ) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_fully_negotiated_outbound(protocol, info)
        }
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_event(event)
        }
    }

    #[inline]
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_dial_upgrade_error(info, error)
        }
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_inbound_closed()
        }
    }

    #[inline]
    fn connection_keep_alive(&self) -> KeepAlive {
        if let Some(inner) = self.inner.as_ref() {
            inner.connection_keep_alive()
        } else {
            KeepAlive::Now
        }
    }

    #[inline]
    fn shutdown(&mut self) {
        self.inner = match mem::replace(&mut self.inner, State::Shutdown) {
            State::Normal(mut inner) => {
                inner.shutdown();
                State::ShuttingDown(inner)
            },
            s @ State::ShuttingDown(_) => s,
            s @ State::Shutdown => s,
        };
    }

    #[inline]
    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        Self::Error,
    > {
        let poll = match self.inner.as_mut() {
            Some(i) => i.poll(),
            None => return Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown)),
        };

        if let Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown)) = poll {
            self.inner = State::Shutdown;
        }

        poll
    }
}
