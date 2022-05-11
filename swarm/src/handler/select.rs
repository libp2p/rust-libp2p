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

use crate::handler::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, IntoConnectionHandler,
    KeepAlive, SubstreamProtocol,
};
use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend, SendWrapper};

use libp2p_core::{
    either::{EitherError, EitherOutput},
    upgrade::{EitherUpgrade, NegotiationError, ProtocolError, SelectUpgrade, UpgradeError},
    ConnectedPoint, Multiaddr, PeerId,
};
use std::{cmp, task::Context, task::Poll};

/// Implementation of `IntoConnectionHandler` that combines two protocols into one.
#[derive(Debug, Clone)]
pub struct IntoConnectionHandlerSelect<TProto1, TProto2> {
    /// The first protocol.
    proto1: TProto1,
    /// The second protocol.
    proto2: TProto2,
}

impl<TProto1, TProto2> IntoConnectionHandlerSelect<TProto1, TProto2> {
    /// Builds a `IntoConnectionHandlerSelect`.
    pub(crate) fn new(proto1: TProto1, proto2: TProto2) -> Self {
        IntoConnectionHandlerSelect { proto1, proto2 }
    }

    pub fn into_inner(self) -> (TProto1, TProto2) {
        (self.proto1, self.proto2)
    }
}

impl<TProto1, TProto2> IntoConnectionHandler for IntoConnectionHandlerSelect<TProto1, TProto2>
where
    TProto1: IntoConnectionHandler,
    TProto2: IntoConnectionHandler,
{
    type Handler = ConnectionHandlerSelect<TProto1::Handler, TProto2::Handler>;

    fn into_handler(
        self,
        remote_peer_id: &PeerId,
        connected_point: &ConnectedPoint,
    ) -> Self::Handler {
        ConnectionHandlerSelect {
            proto1: self.proto1.into_handler(remote_peer_id, connected_point),
            proto2: self.proto2.into_handler(remote_peer_id, connected_point),
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
        SelectUpgrade::new(
            SendWrapper(self.proto1.inbound_protocol()),
            SendWrapper(self.proto2.inbound_protocol()),
        )
    }
}

/// Implementation of [`ConnectionHandler`] that combines two protocols into one.
#[derive(Debug, Clone)]
pub struct ConnectionHandlerSelect<TProto1, TProto2> {
    /// The first protocol.
    proto1: TProto1,
    /// The second protocol.
    proto2: TProto2,
}

impl<TProto1, TProto2> ConnectionHandlerSelect<TProto1, TProto2> {
    /// Builds a [`ConnectionHandlerSelect`].
    pub(crate) fn new(proto1: TProto1, proto2: TProto2) -> Self {
        ConnectionHandlerSelect { proto1, proto2 }
    }

    pub fn into_inner(self) -> (TProto1, TProto2) {
        (self.proto1, self.proto2)
    }
}

impl<TProto1, TProto2> ConnectionHandler for ConnectionHandlerSelect<TProto1, TProto2>
where
    TProto1: ConnectionHandler,
    TProto2: ConnectionHandler,
{
    type InEvent = EitherOutput<TProto1::InEvent, TProto2::InEvent>;
    type OutEvent = EitherOutput<TProto1::OutEvent, TProto2::OutEvent>;
    type Error = EitherError<TProto1::Error, TProto2::Error>;
    type InboundProtocol = SelectUpgrade<
        SendWrapper<<TProto1 as ConnectionHandler>::InboundProtocol>,
        SendWrapper<<TProto2 as ConnectionHandler>::InboundProtocol>,
    >;
    type OutboundProtocol = EitherUpgrade<
        SendWrapper<TProto1::OutboundProtocol>,
        SendWrapper<TProto2::OutboundProtocol>,
    >;
    type OutboundOpenInfo = EitherOutput<TProto1::OutboundOpenInfo, TProto2::OutboundOpenInfo>;
    type InboundOpenInfo = (TProto1::InboundOpenInfo, TProto2::InboundOpenInfo);

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        let proto1 = self.proto1.listen_protocol();
        let proto2 = self.proto2.listen_protocol();
        let timeout = *std::cmp::max(proto1.timeout(), proto2.timeout());
        let (u1, i1) = proto1.into_upgrade();
        let (u2, i2) = proto2.into_upgrade();
        let choice = SelectUpgrade::new(SendWrapper(u1), SendWrapper(u2));
        SubstreamProtocol::new(choice, (i1, i2)).with_timeout(timeout)
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        endpoint: Self::OutboundOpenInfo,
    ) {
        match (protocol, endpoint) {
            (EitherOutput::First(protocol), EitherOutput::First(info)) => {
                self.proto1.inject_fully_negotiated_outbound(protocol, info)
            }
            (EitherOutput::Second(protocol), EitherOutput::Second(info)) => {
                self.proto2.inject_fully_negotiated_outbound(protocol, info)
            }
            (EitherOutput::First(_), EitherOutput::Second(_)) => {
                panic!("wrong API usage: the protocol doesn't match the upgrade info")
            }
            (EitherOutput::Second(_), EitherOutput::First(_)) => {
                panic!("wrong API usage: the protocol doesn't match the upgrade info")
            }
        }
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        (i1, i2): Self::InboundOpenInfo,
    ) {
        match protocol {
            EitherOutput::First(protocol) => {
                self.proto1.inject_fully_negotiated_inbound(protocol, i1)
            }
            EitherOutput::Second(protocol) => {
                self.proto2.inject_fully_negotiated_inbound(protocol, i2)
            }
        }
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            EitherOutput::First(event) => self.proto1.inject_event(event),
            EitherOutput::Second(event) => self.proto2.inject_event(event),
        }
    }

    fn inject_address_change(&mut self, new_address: &Multiaddr) {
        self.proto1.inject_address_change(new_address);
        self.proto2.inject_address_change(new_address)
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ConnectionHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        match (info, error) {
            (EitherOutput::First(info), ConnectionHandlerUpgrErr::Timer) => self
                .proto1
                .inject_dial_upgrade_error(info, ConnectionHandlerUpgrErr::Timer),
            (EitherOutput::First(info), ConnectionHandlerUpgrErr::Timeout) => self
                .proto1
                .inject_dial_upgrade_error(info, ConnectionHandlerUpgrErr::Timeout),
            (
                EitherOutput::First(info),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(err)),
            ) => self.proto1.inject_dial_upgrade_error(
                info,
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(err)),
            ),
            (
                EitherOutput::First(info),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(err))),
            ) => self.proto1.inject_dial_upgrade_error(
                info,
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(err)),
            ),
            (
                EitherOutput::First(_),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(_))),
            ) => {
                panic!("Wrong API usage; the upgrade error doesn't match the outbound open info");
            }
            (EitherOutput::Second(info), ConnectionHandlerUpgrErr::Timeout) => self
                .proto2
                .inject_dial_upgrade_error(info, ConnectionHandlerUpgrErr::Timeout),
            (EitherOutput::Second(info), ConnectionHandlerUpgrErr::Timer) => self
                .proto2
                .inject_dial_upgrade_error(info, ConnectionHandlerUpgrErr::Timer),
            (
                EitherOutput::Second(info),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(err)),
            ) => self.proto2.inject_dial_upgrade_error(
                info,
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(err)),
            ),
            (
                EitherOutput::Second(info),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(err))),
            ) => self.proto2.inject_dial_upgrade_error(
                info,
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(err)),
            ),
            (
                EitherOutput::Second(_),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(_))),
            ) => {
                panic!("Wrong API usage; the upgrade error doesn't match the outbound open info");
            }
        }
    }

    fn inject_listen_upgrade_error(
        &mut self,
        (i1, i2): Self::InboundOpenInfo,
        error: ConnectionHandlerUpgrErr<<Self::InboundProtocol as InboundUpgradeSend>::Error>,
    ) {
        match error {
            ConnectionHandlerUpgrErr::Timer => {
                self.proto1
                    .inject_listen_upgrade_error(i1, ConnectionHandlerUpgrErr::Timer);
                self.proto2
                    .inject_listen_upgrade_error(i2, ConnectionHandlerUpgrErr::Timer)
            }
            ConnectionHandlerUpgrErr::Timeout => {
                self.proto1
                    .inject_listen_upgrade_error(i1, ConnectionHandlerUpgrErr::Timeout);
                self.proto2
                    .inject_listen_upgrade_error(i2, ConnectionHandlerUpgrErr::Timeout)
            }
            ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(NegotiationError::Failed)) => {
                self.proto1.inject_listen_upgrade_error(
                    i1,
                    ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(
                        NegotiationError::Failed,
                    )),
                );
                self.proto2.inject_listen_upgrade_error(
                    i2,
                    ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(
                        NegotiationError::Failed,
                    )),
                );
            }
            ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(
                NegotiationError::ProtocolError(e),
            )) => {
                let (e1, e2);
                match e {
                    ProtocolError::IoError(e) => {
                        e1 = NegotiationError::ProtocolError(ProtocolError::IoError(
                            e.kind().into(),
                        ));
                        e2 = NegotiationError::ProtocolError(ProtocolError::IoError(e))
                    }
                    ProtocolError::InvalidMessage => {
                        e1 = NegotiationError::ProtocolError(ProtocolError::InvalidMessage);
                        e2 = NegotiationError::ProtocolError(ProtocolError::InvalidMessage)
                    }
                    ProtocolError::InvalidProtocol => {
                        e1 = NegotiationError::ProtocolError(ProtocolError::InvalidProtocol);
                        e2 = NegotiationError::ProtocolError(ProtocolError::InvalidProtocol)
                    }
                    ProtocolError::TooManyProtocols => {
                        e1 = NegotiationError::ProtocolError(ProtocolError::TooManyProtocols);
                        e2 = NegotiationError::ProtocolError(ProtocolError::TooManyProtocols)
                    }
                }
                self.proto1.inject_listen_upgrade_error(
                    i1,
                    ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(e1)),
                );
                self.proto2.inject_listen_upgrade_error(
                    i2,
                    ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(e2)),
                )
            }
            ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::A(e))) => {
                self.proto1.inject_listen_upgrade_error(
                    i1,
                    ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)),
                )
            }
            ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(EitherError::B(e))) => {
                self.proto2.inject_listen_upgrade_error(
                    i2,
                    ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)),
                )
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        cmp::max(
            self.proto1.connection_keep_alive(),
            self.proto2.connection_keep_alive(),
        )
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
        match self.proto1.poll(cx) {
            Poll::Ready(ConnectionHandlerEvent::Custom(event)) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(EitherOutput::First(event)));
            }
            Poll::Ready(ConnectionHandlerEvent::Close(event)) => {
                return Poll::Ready(ConnectionHandlerEvent::Close(EitherError::A(event)));
            }
            Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol }) => {
                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol
                        .map_upgrade(|u| EitherUpgrade::A(SendWrapper(u)))
                        .map_info(EitherOutput::First),
                });
            }
            Poll::Pending => (),
        };

        match self.proto2.poll(cx) {
            Poll::Ready(ConnectionHandlerEvent::Custom(event)) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(EitherOutput::Second(event)));
            }
            Poll::Ready(ConnectionHandlerEvent::Close(event)) => {
                return Poll::Ready(ConnectionHandlerEvent::Close(EitherError::B(event)));
            }
            Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol }) => {
                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol
                        .map_upgrade(|u| EitherUpgrade::B(SendWrapper(u)))
                        .map_info(EitherOutput::Second),
                });
            }
            Poll::Pending => (),
        };

        Poll::Pending
    }
}
