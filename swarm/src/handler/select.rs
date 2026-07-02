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

use std::{
    cmp,
    collections::HashMap,
    task::{Context, Poll},
};

use either::Either;
use futures::{future, ready};
use libp2p_core::upgrade::SelectUpgrade;

use crate::{
    handler::{
        AddressChange, ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent,
        DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound, InboundUpgradeSend,
        ListenUpgradeError, OutboundUpgradeSend, StreamUpgradeError, SubstreamProtocol,
    },
    upgrade::SendWrapper,
};

/// Which child of a [`ConnectionHandlerSelect`] owns a datagram control stream.
#[derive(Debug, Clone, Copy)]
enum Side {
    First,
    Second,
}

/// Implementation of [`ConnectionHandler`] that combines two protocols into one.
#[derive(Debug, Clone)]
pub struct ConnectionHandlerSelect<TProto1, TProto2> {
    /// The first protocol.
    proto1: TProto1,
    /// The second protocol.
    proto2: TProto2,
    /// Control-stream id -> owning child, for routing inbound datagrams.
    datagram_routes: HashMap<u64, Side>,
}

impl<TProto1, TProto2> ConnectionHandlerSelect<TProto1, TProto2> {
    /// Builds a [`ConnectionHandlerSelect`].
    pub(crate) fn new(proto1: TProto1, proto2: TProto2) -> Self {
        ConnectionHandlerSelect {
            proto1,
            proto2,
            datagram_routes: HashMap::new(),
        }
    }

    pub fn into_inner(self) -> (TProto1, TProto2) {
        (self.proto1, self.proto2)
    }
}

impl<S1OOI, S2OOI, S1OP, S2OP>
    FullyNegotiatedOutbound<Either<SendWrapper<S1OP>, SendWrapper<S2OP>>, Either<S1OOI, S2OOI>>
where
    S1OP: OutboundUpgradeSend,
    S2OP: OutboundUpgradeSend,
    S1OOI: Send + 'static,
    S2OOI: Send + 'static,
{
    pub(crate) fn transpose(
        self,
    ) -> Either<FullyNegotiatedOutbound<S1OP, S1OOI>, FullyNegotiatedOutbound<S2OP, S2OOI>> {
        match self {
            FullyNegotiatedOutbound {
                protocol: future::Either::Left(protocol),
                info: Either::Left(info),
                stream_id,
            } => Either::Left(FullyNegotiatedOutbound {
                protocol,
                info,
                stream_id,
            }),
            FullyNegotiatedOutbound {
                protocol: future::Either::Right(protocol),
                info: Either::Right(info),
                stream_id,
            } => Either::Right(FullyNegotiatedOutbound {
                protocol,
                info,
                stream_id,
            }),
            _ => panic!("wrong API usage: the protocol doesn't match the upgrade info"),
        }
    }
}

impl<S1IP, S1IOI, S2IP, S2IOI>
    FullyNegotiatedInbound<SelectUpgrade<SendWrapper<S1IP>, SendWrapper<S2IP>>, (S1IOI, S2IOI)>
where
    S1IP: InboundUpgradeSend,
    S2IP: InboundUpgradeSend,
{
    pub(crate) fn transpose(
        self,
    ) -> Either<FullyNegotiatedInbound<S1IP, S1IOI>, FullyNegotiatedInbound<S2IP, S2IOI>> {
        match self {
            FullyNegotiatedInbound {
                protocol: future::Either::Left(protocol),
                info: (i1, _i2),
                stream_id,
            } => Either::Left(FullyNegotiatedInbound {
                protocol,
                info: i1,
                stream_id,
            }),
            FullyNegotiatedInbound {
                protocol: future::Either::Right(protocol),
                info: (_i1, i2),
                stream_id,
            } => Either::Right(FullyNegotiatedInbound {
                protocol,
                info: i2,
                stream_id,
            }),
        }
    }
}

impl<S1OOI, S2OOI, S1OP, S2OP>
    DialUpgradeError<Either<S1OOI, S2OOI>, Either<SendWrapper<S1OP>, SendWrapper<S2OP>>>
where
    S1OP: OutboundUpgradeSend,
    S2OP: OutboundUpgradeSend,
    S1OOI: Send + 'static,
    S2OOI: Send + 'static,
{
    pub(crate) fn transpose(
        self,
    ) -> Either<DialUpgradeError<S1OOI, S1OP>, DialUpgradeError<S2OOI, S2OP>> {
        match self {
            DialUpgradeError {
                info: Either::Left(info),
                error: StreamUpgradeError::Apply(Either::Left(err)),
            } => Either::Left(DialUpgradeError {
                info,
                error: StreamUpgradeError::Apply(err),
            }),
            DialUpgradeError {
                info: Either::Right(info),
                error: StreamUpgradeError::Apply(Either::Right(err)),
            } => Either::Right(DialUpgradeError {
                info,
                error: StreamUpgradeError::Apply(err),
            }),
            DialUpgradeError {
                info: Either::Left(info),
                error: e,
            } => Either::Left(DialUpgradeError {
                info,
                error: e.map_upgrade_err(|_| panic!("already handled above")),
            }),
            DialUpgradeError {
                info: Either::Right(info),
                error: e,
            } => Either::Right(DialUpgradeError {
                info,
                error: e.map_upgrade_err(|_| panic!("already handled above")),
            }),
        }
    }
}

impl<TProto1, TProto2> ConnectionHandlerSelect<TProto1, TProto2>
where
    TProto1: ConnectionHandler,
    TProto2: ConnectionHandler,
{
    fn on_listen_upgrade_error(
        &mut self,
        ListenUpgradeError {
            info: (i1, i2),
            error,
        }: ListenUpgradeError<
            <Self as ConnectionHandler>::InboundOpenInfo,
            <Self as ConnectionHandler>::InboundProtocol,
        >,
    ) {
        match error {
            Either::Left(error) => {
                self.proto1
                    .on_connection_event(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                        info: i1,
                        error,
                    }));
            }
            Either::Right(error) => {
                self.proto2
                    .on_connection_event(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
                        info: i2,
                        error,
                    }));
            }
        }
    }
}

impl<TProto1, TProto2> ConnectionHandler for ConnectionHandlerSelect<TProto1, TProto2>
where
    TProto1: ConnectionHandler,
    TProto2: ConnectionHandler,
{
    type FromBehaviour = Either<TProto1::FromBehaviour, TProto2::FromBehaviour>;
    type ToBehaviour = Either<TProto1::ToBehaviour, TProto2::ToBehaviour>;
    type InboundProtocol = SelectUpgrade<
        SendWrapper<<TProto1 as ConnectionHandler>::InboundProtocol>,
        SendWrapper<<TProto2 as ConnectionHandler>::InboundProtocol>,
    >;
    type OutboundProtocol =
        Either<SendWrapper<TProto1::OutboundProtocol>, SendWrapper<TProto2::OutboundProtocol>>;
    type OutboundOpenInfo = Either<TProto1::OutboundOpenInfo, TProto2::OutboundOpenInfo>;
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

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            Either::Left(event) => self.proto1.on_behaviour_event(event),
            Either::Right(event) => self.proto2.on_behaviour_event(event),
        }
    }

    fn connection_keep_alive(&self) -> bool {
        cmp::max(
            self.proto1.connection_keep_alive(),
            self.proto2.connection_keep_alive(),
        )
    }

    fn supports_datagrams(&self, protocol: &crate::StreamProtocol) -> bool {
        self.proto1.supports_datagrams(protocol) || self.proto2.supports_datagrams(protocol)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        match self.proto1.poll(cx) {
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event)) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Left(event)));
            }
            Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol }) => {
                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol
                        .map_upgrade(|u| Either::Left(SendWrapper(u)))
                        .map_info(Either::Left),
                });
            }
            Poll::Ready(ConnectionHandlerEvent::ReportRemoteProtocols(support)) => {
                return Poll::Ready(ConnectionHandlerEvent::ReportRemoteProtocols(support));
            }
            Poll::Ready(ConnectionHandlerEvent::SendDatagram(data)) => {
                return Poll::Ready(ConnectionHandlerEvent::SendDatagram(data));
            }
            Poll::Pending => (),
        };

        match self.proto2.poll(cx) {
            Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event)) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Right(
                    event,
                )));
            }
            Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol }) => {
                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol
                        .map_upgrade(|u| Either::Right(SendWrapper(u)))
                        .map_info(Either::Right),
                });
            }
            Poll::Ready(ConnectionHandlerEvent::ReportRemoteProtocols(support)) => {
                return Poll::Ready(ConnectionHandlerEvent::ReportRemoteProtocols(support));
            }
            Poll::Ready(ConnectionHandlerEvent::SendDatagram(data)) => {
                return Poll::Ready(ConnectionHandlerEvent::SendDatagram(data));
            }
            Poll::Pending => (),
        };

        Poll::Pending
    }

    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Option<Self::ToBehaviour>> {
        if let Some(e) = ready!(self.proto1.poll_close(cx)) {
            return Poll::Ready(Some(Either::Left(e)));
        }

        if let Some(e) = ready!(self.proto2.poll_close(cx)) {
            return Poll::Ready(Some(Either::Right(e)));
        }

        Poll::Ready(None)
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
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                match fully_negotiated_outbound.transpose() {
                    Either::Left(f) => {
                        if let Some(id) = f.stream_id {
                            self.datagram_routes.insert(id, Side::First);
                        }
                        self.proto1
                            .on_connection_event(ConnectionEvent::FullyNegotiatedOutbound(f));
                    }
                    Either::Right(f) => {
                        if let Some(id) = f.stream_id {
                            self.datagram_routes.insert(id, Side::Second);
                        }
                        self.proto2
                            .on_connection_event(ConnectionEvent::FullyNegotiatedOutbound(f));
                    }
                }
            }
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                match fully_negotiated_inbound.transpose() {
                    Either::Left(f) => {
                        if let Some(id) = f.stream_id {
                            self.datagram_routes.insert(id, Side::First);
                        }
                        self.proto1
                            .on_connection_event(ConnectionEvent::FullyNegotiatedInbound(f));
                    }
                    Either::Right(f) => {
                        if let Some(id) = f.stream_id {
                            self.datagram_routes.insert(id, Side::Second);
                        }
                        self.proto2
                            .on_connection_event(ConnectionEvent::FullyNegotiatedInbound(f));
                    }
                }
            }
            ConnectionEvent::AddressChange(address) => {
                self.proto1
                    .on_connection_event(ConnectionEvent::AddressChange(AddressChange {
                        new_address: address.new_address,
                    }));

                self.proto2
                    .on_connection_event(ConnectionEvent::AddressChange(AddressChange {
                        new_address: address.new_address,
                    }));
            }
            ConnectionEvent::Datagram(datagram) => {
                match self.datagram_routes.get(&datagram.stream_id) {
                    Some(Side::First) => {
                        self.proto1
                            .on_connection_event(ConnectionEvent::Datagram(datagram));
                    }
                    Some(Side::Second) => {
                        self.proto2
                            .on_connection_event(ConnectionEvent::Datagram(datagram));
                    }
                    // Unknown control stream; drop.
                    None => {}
                }
            }
            ConnectionEvent::DatagramMaxSize(max) => {
                self.proto1
                    .on_connection_event(ConnectionEvent::DatagramMaxSize(max));
                self.proto2
                    .on_connection_event(ConnectionEvent::DatagramMaxSize(max));
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                match dial_upgrade_error.transpose() {
                    Either::Left(err) => self
                        .proto1
                        .on_connection_event(ConnectionEvent::DialUpgradeError(err)),
                    Either::Right(err) => self
                        .proto2
                        .on_connection_event(ConnectionEvent::DialUpgradeError(err)),
                }
            }
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            }
            ConnectionEvent::LocalProtocolsChange(supported_protocols) => {
                self.proto1
                    .on_connection_event(ConnectionEvent::LocalProtocolsChange(
                        supported_protocols.clone(),
                    ));
                self.proto2
                    .on_connection_event(ConnectionEvent::LocalProtocolsChange(
                        supported_protocols,
                    ));
            }
            ConnectionEvent::RemoteProtocolsChange(supported_protocols) => {
                self.proto1
                    .on_connection_event(ConnectionEvent::RemoteProtocolsChange(
                        supported_protocols.clone(),
                    ));
                self.proto2
                    .on_connection_event(ConnectionEvent::RemoteProtocolsChange(
                        supported_protocols,
                    ));
            }
        }
    }
}
