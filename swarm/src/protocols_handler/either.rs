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

pub enum IntoEitherHandler<L, R> {
    Left(L),
    Right(R),
}

/// Implementation of a [`IntoProtocolsHandler`] that represents either of two [`IntoProtocolsHandler`]
/// implementations.
impl<L, R> IntoProtocolsHandler for IntoEitherHandler<L, R>
where
    L: IntoProtocolsHandler,
    R: IntoProtocolsHandler,
{
    type Handler = Either<L::Handler, R::Handler>;

    fn into_handler(self, p: &PeerId, c: &ConnectedPoint) -> Self::Handler {
        match self {
            IntoEitherHandler::Left(into_handler) => Either::Left(into_handler.into_handler(p, c)),
            IntoEitherHandler::Right(into_handler) => {
                Either::Right(into_handler.into_handler(p, c))
            }
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        match self {
            IntoEitherHandler::Left(into_handler) => {
                EitherUpgrade::A(SendWrapper(into_handler.inbound_protocol()))
            }
            IntoEitherHandler::Right(into_handler) => {
                EitherUpgrade::B(SendWrapper(into_handler.inbound_protocol()))
            }
        }
    }
}

/// Implementation of a [`ProtocolsHandler`] that represents either of two [`ProtocolsHandler`]
/// implementations.
impl<L, R> ProtocolsHandler for Either<L, R>
where
    L: ProtocolsHandler,
    R: ProtocolsHandler,
{
    type InEvent = Either<L::InEvent, R::InEvent>;
    type OutEvent = Either<L::OutEvent, R::OutEvent>;
    type Error = Either<L::Error, R::Error>;
    type InboundProtocol =
        EitherUpgrade<SendWrapper<L::InboundProtocol>, SendWrapper<R::InboundProtocol>>;
    type OutboundProtocol =
        EitherUpgrade<SendWrapper<L::OutboundProtocol>, SendWrapper<R::OutboundProtocol>>;
    type InboundOpenInfo = Either<L::InboundOpenInfo, R::InboundOpenInfo>;
    type OutboundOpenInfo = Either<L::OutboundOpenInfo, R::OutboundOpenInfo>;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        match self {
            Either::Left(a) => a
                .listen_protocol()
                .map_upgrade(|u| EitherUpgrade::A(SendWrapper(u)))
                .map_info(Either::Left),
            Either::Right(b) => b
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
            (Either::Left(handler), EitherOutput::First(output), Either::Left(info)) => {
                handler.inject_fully_negotiated_outbound(output, info)
            }
            (Either::Right(handler), EitherOutput::Second(output), Either::Right(info)) => {
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
            (Either::Left(handler), EitherOutput::First(output), Either::Left(info)) => {
                handler.inject_fully_negotiated_inbound(output, info)
            }
            (Either::Right(handler), EitherOutput::Second(output), Either::Right(info)) => {
                handler.inject_fully_negotiated_inbound(output, info)
            }
            _ => unreachable!(),
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match (self, event) {
            (Either::Left(handler), Either::Left(event)) => handler.inject_event(event),
            (Either::Right(handler), Either::Right(event)) => handler.inject_event(event),
            _ => unreachable!(),
        }
    }

    fn inject_address_change(&mut self, addr: &Multiaddr) {
        match self {
            Either::Left(handler) => handler.inject_address_change(addr),
            Either::Right(handler) => handler.inject_address_change(addr),
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        match error {
            ProtocolsHandlerUpgrErr::Timer => match (self, info) {
                (Either::Left(handler), Either::Left(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                (Either::Right(handler), Either::Right(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Timeout => match (self, info) {
                (Either::Left(handler), Either::Left(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                (Either::Right(handler), Either::Right(info)) => {
                    handler.inject_dial_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)) => match (self, info) {
                (Either::Left(handler), Either::Left(info)) => {
                    handler.inject_dial_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                (Either::Right(handler), Either::Right(info)) => {
                    handler.inject_dial_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(e))) => {
                match (self, info) {
                    (Either::Left(handler), Either::Left(info)) => {
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
                    (Either::Right(handler), Either::Right(info)) => {
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
                (Either::Left(handler), Either::Left(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                (Either::Right(handler), Either::Right(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timer);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Timeout => match (self, info) {
                (Either::Left(handler), Either::Left(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                (Either::Right(handler), Either::Right(info)) => {
                    handler.inject_listen_upgrade_error(info, ProtocolsHandlerUpgrErr::Timeout);
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)) => match (self, info) {
                (Either::Left(handler), Either::Left(info)) => {
                    handler.inject_listen_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                (Either::Right(handler), Either::Right(info)) => {
                    handler.inject_listen_upgrade_error(
                        info,
                        ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                    );
                }
                _ => unreachable!(),
            },
            ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(e))) => {
                match (self, info) {
                    (Either::Left(handler), Either::Left(info)) => {
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
                    (Either::Right(handler), Either::Right(info)) => {
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
            Either::Left(handler) => handler.connection_keep_alive(),
            Either::Right(handler) => handler.connection_keep_alive(),
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
        let event = match self {
            Either::Left(handler) => futures::ready!(handler.poll(cx))
                .map_custom(Either::Left)
                .map_close(Either::Left)
                .map_protocol(|p| EitherUpgrade::A(SendWrapper(p)))
                .map_outbound_open_info(Either::Left),
            Either::Right(handler) => futures::ready!(handler.poll(cx))
                .map_custom(Either::Right)
                .map_close(Either::Right)
                .map_protocol(|p| EitherUpgrade::B(SendWrapper(p)))
                .map_outbound_open_info(Either::Right),
        };

        Poll::Ready(event)
    }
}
