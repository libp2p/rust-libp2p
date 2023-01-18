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

use crate::handler::{
    ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr,
    DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound, InboundUpgradeSend,
    IntoConnectionHandler, KeepAlive, ListenUpgradeError, OutboundUpgradeSend, SubstreamProtocol,
};
use crate::upgrade::SendWrapper;
use either::Either;
use libp2p_core::either::EitherOutput;
use libp2p_core::upgrade::{EitherUpgrade, UpgradeError};
use libp2p_core::{ConnectedPoint, PeerId};
use std::task::{Context, Poll};

/// Auxiliary type to allow implementing [`IntoConnectionHandler`]. As [`IntoConnectionHandler`] is
/// already implemented for T, we cannot implement it for Either<A, B>.
pub enum IntoEitherHandler<L, R> {
    Left(L),
    Right(R),
}

/// Implementation of a [`IntoConnectionHandler`] that represents either of two [`IntoConnectionHandler`]
/// implementations.
impl<L, R> IntoConnectionHandler for IntoEitherHandler<L, R>
where
    L: IntoConnectionHandler,
    R: IntoConnectionHandler,
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

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
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

// Taken from https://github.com/bluss/either.
impl<L, R> IntoEitherHandler<L, R> {
    /// Returns the left value.
    pub fn unwrap_left(self) -> L {
        match self {
            IntoEitherHandler::Left(l) => l,
            IntoEitherHandler::Right(_) => {
                panic!("called `IntoEitherHandler::unwrap_left()` on a `Right` value.",)
            }
        }
    }

    /// Returns the right value.
    pub fn unwrap_right(self) -> R {
        match self {
            IntoEitherHandler::Right(r) => r,
            IntoEitherHandler::Left(_) => {
                panic!("called `IntoEitherHandler::unwrap_right()` on a `Left` value.",)
            }
        }
    }
}

impl<LIP, RIP, LIOI, RIOI>
    FullyNegotiatedInbound<EitherUpgrade<SendWrapper<LIP>, SendWrapper<RIP>>, Either<LIOI, RIOI>>
where
    RIP: InboundUpgradeSend,
    LIP: InboundUpgradeSend,
{
    fn transpose(
        self,
    ) -> Either<FullyNegotiatedInbound<LIP, LIOI>, FullyNegotiatedInbound<RIP, RIOI>> {
        match self {
            FullyNegotiatedInbound {
                protocol: EitherOutput::First(protocol),
                info: Either::Left(info),
            } => Either::Left(FullyNegotiatedInbound { protocol, info }),
            FullyNegotiatedInbound {
                protocol: EitherOutput::Second(protocol),
                info: Either::Right(info),
            } => Either::Right(FullyNegotiatedInbound { protocol, info }),
            _ => unreachable!(),
        }
    }
}

impl<LOP, ROP, LOOI, ROOI>
    FullyNegotiatedOutbound<EitherUpgrade<SendWrapper<LOP>, SendWrapper<ROP>>, Either<LOOI, ROOI>>
where
    LOP: OutboundUpgradeSend,
    ROP: OutboundUpgradeSend,
{
    fn transpose(
        self,
    ) -> Either<FullyNegotiatedOutbound<LOP, LOOI>, FullyNegotiatedOutbound<ROP, ROOI>> {
        match self {
            FullyNegotiatedOutbound {
                protocol: EitherOutput::First(protocol),
                info: Either::Left(info),
            } => Either::Left(FullyNegotiatedOutbound { protocol, info }),
            FullyNegotiatedOutbound {
                protocol: EitherOutput::Second(protocol),
                info: Either::Right(info),
            } => Either::Right(FullyNegotiatedOutbound { protocol, info }),
            _ => unreachable!(),
        }
    }
}

impl<LOP, ROP, LOOI, ROOI>
    DialUpgradeError<Either<LOOI, ROOI>, EitherUpgrade<SendWrapper<LOP>, SendWrapper<ROP>>>
where
    LOP: OutboundUpgradeSend,
    ROP: OutboundUpgradeSend,
{
    fn transpose(self) -> Either<DialUpgradeError<LOOI, LOP>, DialUpgradeError<ROOI, ROP>> {
        match self {
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(Either::Left(error))),
                info: Either::Left(info),
            } => Either::Left(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            }),
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(Either::Right(error))),
                info: Either::Right(info),
            } => Either::Right(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            }),
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Left(info),
            } => Either::Left(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            }),
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Right(info),
            } => Either::Right(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            }),
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Left(info),
            } => Either::Left(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            }),
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Right(info),
            } => Either::Right(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            }),
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Left(info),
            } => Either::Left(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            }),
            DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Right(info),
            } => Either::Right(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            }),
            _ => unreachable!(),
        }
    }
}

impl<LIP, RIP, LIOI, RIOI>
    ListenUpgradeError<Either<LIOI, RIOI>, EitherUpgrade<SendWrapper<LIP>, SendWrapper<RIP>>>
where
    RIP: InboundUpgradeSend,
    LIP: InboundUpgradeSend,
{
    fn transpose(self) -> Either<ListenUpgradeError<LIOI, LIP>, ListenUpgradeError<RIOI, RIP>> {
        match self {
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(Either::Left(error))),
                info: Either::Left(info),
            } => Either::Left(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            }),
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(Either::Right(error))),
                info: Either::Right(info),
            } => Either::Right(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            }),
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Left(info),
            } => Either::Left(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            }),
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Right(info),
            } => Either::Right(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            }),
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Left(info),
            } => Either::Left(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            }),
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Right(info),
            } => Either::Right(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            }),
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Left(info),
            } => Either::Left(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            }),
            ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Right(info),
            } => Either::Right(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            }),
            _ => unreachable!(),
        }
    }
}

/// Implementation of a [`ConnectionHandler`] that represents either of two [`ConnectionHandler`]
/// implementations.
impl<L, R> ConnectionHandler for Either<L, R>
where
    L: ConnectionHandler,
    R: ConnectionHandler,
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

    fn on_behaviour_event(&mut self, event: Self::InEvent) {
        match (self, event) {
            (Either::Left(handler), Either::Left(event)) => handler.on_behaviour_event(event),
            (Either::Right(handler), Either::Right(event)) => handler.on_behaviour_event(event),
            _ => unreachable!(),
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
        ConnectionHandlerEvent<
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
                match (fully_negotiated_inbound.transpose(), self) {
                    (Either::Left(fully_negotiated_inbound), Either::Left(handler)) => handler
                        .on_connection_event(ConnectionEvent::FullyNegotiatedInbound(
                            fully_negotiated_inbound,
                        )),
                    (Either::Right(fully_negotiated_inbound), Either::Right(handler)) => handler
                        .on_connection_event(ConnectionEvent::FullyNegotiatedInbound(
                            fully_negotiated_inbound,
                        )),
                    _ => unreachable!(),
                }
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                match (fully_negotiated_outbound.transpose(), self) {
                    (Either::Left(fully_negotiated_outbound), Either::Left(handler)) => handler
                        .on_connection_event(ConnectionEvent::FullyNegotiatedOutbound(
                            fully_negotiated_outbound,
                        )),
                    (Either::Right(fully_negotiated_outbound), Either::Right(handler)) => handler
                        .on_connection_event(ConnectionEvent::FullyNegotiatedOutbound(
                            fully_negotiated_outbound,
                        )),
                    _ => unreachable!(),
                }
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                match (dial_upgrade_error.transpose(), self) {
                    (Either::Left(dial_upgrade_error), Either::Left(handler)) => handler
                        .on_connection_event(ConnectionEvent::DialUpgradeError(dial_upgrade_error)),
                    (Either::Right(dial_upgrade_error), Either::Right(handler)) => handler
                        .on_connection_event(ConnectionEvent::DialUpgradeError(dial_upgrade_error)),
                    _ => unreachable!(),
                }
            }
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                match (listen_upgrade_error.transpose(), self) {
                    (Either::Left(listen_upgrade_error), Either::Left(handler)) => handler
                        .on_connection_event(ConnectionEvent::ListenUpgradeError(
                            listen_upgrade_error,
                        )),
                    (Either::Right(listen_upgrade_error), Either::Right(handler)) => handler
                        .on_connection_event(ConnectionEvent::ListenUpgradeError(
                            listen_upgrade_error,
                        )),
                    _ => unreachable!(),
                }
            }
            ConnectionEvent::AddressChange(address_change) => match self {
                Either::Left(handler) => {
                    handler.on_connection_event(ConnectionEvent::AddressChange(address_change))
                }
                Either::Right(handler) => {
                    handler.on_connection_event(ConnectionEvent::AddressChange(address_change))
                }
            },
        }
    }
}
