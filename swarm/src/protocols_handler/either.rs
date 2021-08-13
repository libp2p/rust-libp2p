// Copyright 2021 Protocol Labs.
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
    IntoProtocolsHandler, KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend, SendWrapper};
use either::Either;
use libp2p_core::either::{EitherError, EitherOutput};
use libp2p_core::upgrade::{EitherUpgrade, UpgradeError};
use libp2p_core::{ConnectedPoint, Multiaddr, PeerId};
use std::task::{Context, Poll};

pub enum IntoEitherHandler<A, B> {
    A(A),
    B(B),
}

/// Implementation of a [`IntoProtocolsHandler`] that represents either of two [`IntoProtocolsHandler`]
/// implementations.
impl<A, B> IntoProtocolsHandler for IntoEitherHandler<A, B>
where
    A: IntoProtocolsHandler,
    B: IntoProtocolsHandler,
{
    type Handler = EitherHandler<A::Handler, B::Handler>;

    fn into_handler(self, p: &PeerId, c: &ConnectedPoint) -> Self::Handler {
        match self {
            IntoEitherHandler::A(into_handler) => EitherHandler::A(into_handler.into_handler(p, c)),
            IntoEitherHandler::B(into_handler) => EitherHandler::B(into_handler.into_handler(p, c)),
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        match self {
            IntoEitherHandler::A(into_handler) => {
                EitherUpgrade::A(SendWrapper(into_handler.inbound_protocol()))
            }
            IntoEitherHandler::B(into_handler) => {
                EitherUpgrade::B(SendWrapper(into_handler.inbound_protocol()))
            }
        }
    }
}

/// Implementation of a [`ProtocolsHandler`] that represents either of two [`ProtocolsHandler`]
/// implementations.
#[derive(Debug)]
pub enum EitherHandler<A, B> {
    A(A),
    B(B),
}

impl<A, B> ProtocolsHandler for EitherHandler<A, B>
where
    A: ProtocolsHandler,
    B: ProtocolsHandler,
{
    type InEvent = Either<A::InEvent, B::InEvent>;
    type OutEvent = Either<A::OutEvent, B::OutEvent>;
    type Error = Either<A::Error, B::Error>;
    type InboundProtocol =
        EitherUpgrade<SendWrapper<A::InboundProtocol>, SendWrapper<B::InboundProtocol>>;
    type OutboundProtocol =
        EitherUpgrade<SendWrapper<A::OutboundProtocol>, SendWrapper<B::OutboundProtocol>>;
    type InboundOpenInfo = Either<A::InboundOpenInfo, B::InboundOpenInfo>;
    type OutboundOpenInfo = Either<A::OutboundOpenInfo, B::OutboundOpenInfo>;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        match self {
            EitherHandler::A(a) => a
                .listen_protocol()
                .map_upgrade(|u| EitherUpgrade::A(SendWrapper(u)))
                .map_info(Either::Left),
            EitherHandler::B(b) => b
                .listen_protocol()
                .map_upgrade(|u| EitherUpgrade::B(SendWrapper(u)))
                .map_info(Either::Right),
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        output: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        match (self, output, info) {
            (EitherHandler::A(handler), EitherOutput::First(output), Either::Left(info)) => {
                handler.inject_fully_negotiated_outbound(output, info)
            }
            (EitherHandler::B(handler), EitherOutput::Second(output), Either::Right(info)) => {
                handler.inject_fully_negotiated_outbound(output, info)
            }
            _ => unreachable!(),
        }
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        output: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        info: Self::InboundOpenInfo,
    ) {
        match (self, output, info) {
            (EitherHandler::A(handler), EitherOutput::First(output), Either::Left(info)) => {
                handler.inject_fully_negotiated_inbound(output, info)
            }
            (EitherHandler::B(handler), EitherOutput::Second(output), Either::Right(info)) => {
                handler.inject_fully_negotiated_inbound(output, info)
            }
            _ => unreachable!(),
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match (self, event) {
            (EitherHandler::A(handler), Either::Left(event)) => handler.inject_event(event),
            (EitherHandler::B(handler), Either::Right(event)) => handler.inject_event(event),
            _ => unreachable!(),
        }
    }

    fn inject_address_change(&mut self, addr: &Multiaddr) {
        match self {
            EitherHandler::A(handler) => handler.inject_address_change(addr),
            EitherHandler::B(handler) => handler.inject_address_change(addr),
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        match error {
            ProtocolsHandlerUpgrErr::Timer => match (self, info) {
                (EitherHandler::A(handler), Either::Left(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                (EitherHandler::B(handler), Either::Right(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Timeout => match (self, info) {
                (EitherHandler::A(handler), Either::Left(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                (EitherHandler::B(handler), Either::Right(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)) => match (self, info) {
                (EitherHandler::A(handler), Either::Left(info)) => {
                    handler.inject_dial_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                (EitherHandler::B(handler), Either::Right(info)) => {
                    handler.inject_dial_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(e))) => {
                match (self, info) {
                    (EitherHandler::A(handler), Either::Left(info)) => {
                        handler.inject_dial_upgrade_error(
                            info,
                            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)),
                        );
                    }
                    _ => unreachable!(),
                }
            }
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(e))) => {
                match (self, info) {
                    (EitherHandler::B(handler), Either::Right(info)) => {
                        handler.inject_dial_upgrade_error(
                            info,
                            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)),
                        );
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    fn inject_listen_upgrade_error(
        &mut self,
        info: Self::InboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<<Self::InboundProtocol as InboundUpgradeSend>::Error>,
    ) {
        match error {
            ProtocolsHandlerUpgrErr::Timer => match (self, info) {
                (EitherHandler::A(handler), Either::Left(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                (EitherHandler::B(handler), Either::Right(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Timeout => match (self, info) {
                (EitherHandler::A(handler), Either::Left(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                (EitherHandler::B(handler), Either::Right(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)) => match (self, info) {
                (EitherHandler::A(handler), Either::Left(info)) => {
                    handler.inject_listen_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                (EitherHandler::B(handler), Either::Right(info)) => {
                    handler.inject_listen_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(e))) => {
                match (self, info) {
                    (EitherHandler::A(handler), Either::Left(info)) => {
                        handler.inject_listen_upgrade_error(
                            info,
                            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)),
                        );
                    }
                    _ => unreachable!(),
                }
            }
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(e))) => {
                match (self, info) {
                    (EitherHandler::B(handler), Either::Right(info)) => {
                        handler.inject_listen_upgrade_error(
                            info,
                            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)),
                        );
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        match self {
            EitherHandler::A(handler) => handler.connection_keep_alive(),
            EitherHandler::B(handler) => handler.connection_keep_alive(),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        match self {
            EitherHandler::A(handler) => match handler.poll(cx) {
                Poll::Ready(ProtocolsHandlerEvent::Custom(event)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(Either::Left(event)));
                }
                Poll::Ready(ProtocolsHandlerEvent::Close(event)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Close(Either::Left(event)));
                }
                Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol }) => {
                    return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: protocol
                            .map_upgrade(|u| EitherUpgrade::A(SendWrapper(u)))
                            .map_info(Either::Left),
                    });
                }
                Poll::Pending => Poll::Pending,
            },
            EitherHandler::B(handler) => match handler.poll(cx) {
                Poll::Ready(ProtocolsHandlerEvent::Custom(event)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(Either::Right(event)));
                }
                Poll::Ready(ProtocolsHandlerEvent::Close(event)) => {
                    return Poll::Ready(ProtocolsHandlerEvent::Close(Either::Right(event)));
                }
                Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol }) => {
                    return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: protocol
                            .map_upgrade(|u| EitherUpgrade::B(SendWrapper(u)))
                            .map_info(Either::Right),
                    });
                }
                Poll::Pending => Poll::Pending,
            },
        }
    }
}
