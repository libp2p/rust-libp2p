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
use libp2p_core::either::{EitherError, EitherOutput};
use libp2p_core::upgrade::{EitherUpgrade, UpgradeError};
use libp2p_core::{ConnectedPoint, PeerId};
use std::task::{Context, Poll};

use super::AddressChange;

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

impl<'a, RIP, LIP, LOP, ROP, LIOP, RIOP, LOOP, ROOP>
    ConnectionEvent<
        'a,
        EitherUpgrade<SendWrapper<LIP>, SendWrapper<RIP>>,
        EitherUpgrade<SendWrapper<LOP>, SendWrapper<ROP>>,
        Either<LIOP, RIOP>,
        Either<LOOP, ROOP>,
    >
where
    RIP: InboundUpgradeSend,
    LIP: InboundUpgradeSend,
    LOP: OutboundUpgradeSend,
    ROP: OutboundUpgradeSend,
{
    pub fn transpose(
        self,
    ) -> Either<ConnectionEvent<'a, LIP, LOP, LIOP, LOOP>, ConnectionEvent<'a, RIP, ROP, RIOP, ROOP>>
    {
        match self {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: EitherOutput::First(protocol),
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::FullyNegotiatedInbound(
                FullyNegotiatedInbound { protocol, info },
            )),
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: EitherOutput::Second(protocol),
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::FullyNegotiatedInbound(
                FullyNegotiatedInbound { protocol, info },
            )),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: EitherOutput::First(protocol),
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::FullyNegotiatedOutbound(
                FullyNegotiatedOutbound { protocol, info },
            )),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: EitherOutput::Second(protocol),
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::FullyNegotiatedOutbound(
                FullyNegotiatedOutbound { protocol, info },
            )),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(error))),
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            })),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(error))),
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            })),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            })),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            })),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            })),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            })),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            })),
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(error))),
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(error))),
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(error)),
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(error)),
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timer,
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Left(info),
            }) => Either::Left(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            })),
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info: Either::Right(info),
            }) => Either::Right(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                error: ConnectionHandlerUpgrErr::Timeout,
                info,
            })),
            ConnectionEvent::AddressChange(AddressChange { new_address }) => {
                Either::Left(ConnectionEvent::AddressChange(AddressChange {
                    new_address,
                }))
            }
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
        match (event.transpose(), self) {
            (
                Either::Left(ConnectionEvent::AddressChange(address_change)),
                Either::Left(handler),
            ) => handler.on_connection_event(ConnectionEvent::AddressChange(address_change)),
            (
                Either::Left(ConnectionEvent::AddressChange(address_change)),
                Either::Right(handler),
            ) => handler.on_connection_event(ConnectionEvent::AddressChange(address_change)),
            (Either::Left(event), Either::Left(handler)) => handler.on_connection_event(event),
            (Either::Right(event), Either::Right(handler)) => handler.on_connection_event(event),
            (Either::Left(_), Either::Right(_)) | (Either::Right(_), Either::Left(_)) => {
                unreachable!()
            }
        }
    }
}
