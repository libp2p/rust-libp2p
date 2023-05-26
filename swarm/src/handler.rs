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

//! Once a connection to a remote peer is established, a [`ConnectionHandler`] negotiates
//! and handles one or more specific protocols on the connection.
//!
//! Protocols are negotiated and used on individual substreams of the connection. Thus a
//! [`ConnectionHandler`] defines the inbound and outbound upgrades to apply when creating a new
//! inbound or outbound substream, respectively, and is notified by a [`Swarm`](crate::Swarm) when
//! these upgrades have been successfully applied, including the final output of the upgrade. A
//! [`ConnectionHandler`] can then continue communicating with the peer over the substream using the
//! negotiated protocol(s).
//!
//! Two [`ConnectionHandler`]s can be composed with [`ConnectionHandler::select()`]
//! in order to build a new handler supporting the combined set of protocols,
//! with methods being dispatched to the appropriate handler according to the
//! used protocol(s) determined by the associated types of the handlers.
//!
//! > **Note**: A [`ConnectionHandler`] handles one or more protocols in the context of a single
//! >           connection with a remote. In order to handle a protocol that requires knowledge of
//! >           the network as a whole, see the
//! >           [`NetworkBehaviour`](crate::behaviour::NetworkBehaviour) trait.

pub mod either;
mod map_in;
mod map_out;
pub mod multi;
mod one_shot;
mod pending;
mod select;

pub use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend, SendWrapper, UpgradeInfoSend};
pub use map_in::MapInEvent;
pub use map_out::MapOutEvent;
pub use one_shot::{OneShotHandler, OneShotHandlerConfig};
pub use pending::PendingConnectionHandler;
pub use select::ConnectionHandlerSelect;

use crate::StreamProtocol;
use ::either::Either;
use instant::Instant;
use libp2p_core::Multiaddr;
use once_cell::sync::Lazy;
use smallvec::SmallVec;
use std::collections::hash_map::RandomState;
use std::collections::hash_set::{Difference, Intersection};
use std::collections::HashSet;
use std::iter::Peekable;
use std::{cmp::Ordering, error, fmt, io, task::Context, task::Poll, time::Duration};

/// A handler for a set of protocols used on a connection with a remote.
///
/// This trait should be implemented for a type that maintains the state for
/// the execution of a specific protocol with a remote.
///
/// # Handling a protocol
///
/// Communication with a remote over a set of protocols is initiated in one of two ways:
///
///   1. Dialing by initiating a new outbound substream. In order to do so,
///      [`ConnectionHandler::poll()`] must return an [`ConnectionHandlerEvent::OutboundSubstreamRequest`],
///      providing an instance of [`libp2p_core::upgrade::OutboundUpgrade`] that is used to negotiate the
///      protocol(s). Upon success, [`ConnectionHandler::on_connection_event`] is called with
///      [`ConnectionEvent::FullyNegotiatedOutbound`] translating the final output of the upgrade.
///
///   2. Listening by accepting a new inbound substream. When a new inbound substream
///      is created on a connection, [`ConnectionHandler::listen_protocol`] is called
///      to obtain an instance of [`libp2p_core::upgrade::InboundUpgrade`] that is used to
///      negotiate the protocol(s). Upon success,
///      [`ConnectionHandler::on_connection_event`] is called with [`ConnectionEvent::FullyNegotiatedInbound`]
///      translating the final output of the upgrade.
///
///
/// # Connection Keep-Alive
///
/// A [`ConnectionHandler`] can influence the lifetime of the underlying connection
/// through [`ConnectionHandler::connection_keep_alive`]. That is, the protocol
/// implemented by the handler can include conditions for terminating the connection.
/// The lifetime of successfully negotiated substreams is fully controlled by the handler.
///
/// Implementors of this trait should keep in mind that the connection can be closed at any time.
/// When a connection is closed gracefully, the substreams used by the handler may still
/// continue reading data until the remote closes its side of the connection.
pub trait ConnectionHandler: Send + 'static {
    /// A type representing the message(s) a [`NetworkBehaviour`](crate::behaviour::NetworkBehaviour) can send to a [`ConnectionHandler`] via [`ToSwarm::NotifyHandler`](crate::behaviour::ToSwarm::NotifyHandler)
    type FromBehaviour: fmt::Debug + Send + 'static;
    /// A type representing message(s) a [`ConnectionHandler`] can send to a [`NetworkBehaviour`](crate::behaviour::NetworkBehaviour) via [`ConnectionHandlerEvent::NotifyBehaviour`].
    type ToBehaviour: fmt::Debug + Send + 'static;
    /// The type of errors returned by [`ConnectionHandler::poll`].
    type Error: error::Error + fmt::Debug + Send + 'static;
    /// The inbound upgrade for the protocol(s) used by the handler.
    type InboundProtocol: InboundUpgradeSend;
    /// The outbound upgrade for the protocol(s) used by the handler.
    type OutboundProtocol: OutboundUpgradeSend;
    /// The type of additional information returned from `listen_protocol`.
    type InboundOpenInfo: Send + 'static;
    /// The type of additional information passed to an `OutboundSubstreamRequest`.
    type OutboundOpenInfo: Send + 'static;

    /// The [`InboundUpgrade`](libp2p_core::upgrade::InboundUpgrade) to apply on inbound
    /// substreams to negotiate the desired protocols.
    ///
    /// > **Note**: The returned `InboundUpgrade` should always accept all the generally
    /// >           supported protocols, even if in a specific context a particular one is
    /// >           not supported, (eg. when only allowing one substream at a time for a protocol).
    /// >           This allows a remote to put the list of supported protocols in a cache.
    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo>;

    /// Returns until when the connection should be kept alive.
    ///
    /// This method is called by the `Swarm` after each invocation of
    /// [`ConnectionHandler::poll`] to determine if the connection and the associated
    /// [`ConnectionHandler`]s should be kept alive as far as this handler is concerned
    /// and if so, for how long.
    ///
    /// Returning [`KeepAlive::No`] indicates that the connection should be
    /// closed and this handler destroyed immediately.
    ///
    /// Returning [`KeepAlive::Until`] indicates that the connection may be closed
    /// and this handler destroyed after the specified `Instant`.
    ///
    /// Returning [`KeepAlive::Yes`] indicates that the connection should
    /// be kept alive until the next call to this method.
    ///
    /// > **Note**: The connection is always closed and the handler destroyed
    /// > when [`ConnectionHandler::poll`] returns an error. Furthermore, the
    /// > connection may be closed for reasons outside of the control
    /// > of the handler.
    fn connection_keep_alive(&self) -> KeepAlive;

    /// Should behave like `Stream::poll()`.
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
            Self::Error,
        >,
    >;

    /// Adds a closure that turns the input event into something else.
    fn map_in_event<TNewIn, TMap>(self, map: TMap) -> MapInEvent<Self, TNewIn, TMap>
    where
        Self: Sized,
        TMap: Fn(&TNewIn) -> Option<&Self::FromBehaviour>,
    {
        MapInEvent::new(self, map)
    }

    /// Adds a closure that turns the output event into something else.
    fn map_out_event<TMap, TNewOut>(self, map: TMap) -> MapOutEvent<Self, TMap>
    where
        Self: Sized,
        TMap: FnMut(Self::ToBehaviour) -> TNewOut,
    {
        MapOutEvent::new(self, map)
    }

    /// Creates a new [`ConnectionHandler`] that selects either this handler or
    /// `other` by delegating methods calls appropriately.
    ///
    /// > **Note**: The largest `KeepAlive` returned by the two handlers takes precedence,
    /// > i.e. is returned from [`ConnectionHandler::connection_keep_alive`] by the returned
    /// > handler.
    fn select<TProto2>(self, other: TProto2) -> ConnectionHandlerSelect<Self, TProto2>
    where
        Self: Sized,
    {
        ConnectionHandlerSelect::new(self, other)
    }

    /// Informs the handler about an event from the [`NetworkBehaviour`](super::NetworkBehaviour).
    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour);

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    );
}

/// Enumeration with the list of the possible stream events
/// to pass to [`on_connection_event`](ConnectionHandler::on_connection_event).
pub enum ConnectionEvent<'a, IP: InboundUpgradeSend, OP: OutboundUpgradeSend, IOI, OOI> {
    /// Informs the handler about the output of a successful upgrade on a new inbound substream.
    FullyNegotiatedInbound(FullyNegotiatedInbound<IP, IOI>),
    /// Informs the handler about the output of a successful upgrade on a new outbound stream.
    FullyNegotiatedOutbound(FullyNegotiatedOutbound<OP, OOI>),
    /// Informs the handler about a change in the address of the remote.
    AddressChange(AddressChange<'a>),
    /// Informs the handler that upgrading an outbound substream to the given protocol has failed.
    DialUpgradeError(DialUpgradeError<OOI, OP>),
    /// Informs the handler that upgrading an inbound substream to the given protocol has failed.
    ListenUpgradeError(ListenUpgradeError<IOI, IP>),
    /// The local [`ConnectionHandler`] added or removed support for one or more protocols.
    LocalProtocolsChange(ProtocolsChange<'a>),
    /// The remote [`ConnectionHandler`] now supports a different set of protocols.
    RemoteProtocolsChange(ProtocolsChange<'a>),
}

impl<'a, IP: InboundUpgradeSend, OP: OutboundUpgradeSend, IOI, OOI>
    ConnectionEvent<'a, IP, OP, IOI, OOI>
{
    /// Whether the event concerns an outbound stream.
    pub fn is_outbound(&self) -> bool {
        match self {
            ConnectionEvent::DialUpgradeError(_) | ConnectionEvent::FullyNegotiatedOutbound(_) => {
                true
            }
            ConnectionEvent::FullyNegotiatedInbound(_)
            | ConnectionEvent::AddressChange(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_)
            | ConnectionEvent::ListenUpgradeError(_) => false,
        }
    }

    /// Whether the event concerns an inbound stream.
    pub fn is_inbound(&self) -> bool {
        match self {
            ConnectionEvent::FullyNegotiatedInbound(_) | ConnectionEvent::ListenUpgradeError(_) => {
                true
            }
            ConnectionEvent::FullyNegotiatedOutbound(_)
            | ConnectionEvent::AddressChange(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_)
            | ConnectionEvent::DialUpgradeError(_) => false,
        }
    }
}

/// [`ConnectionEvent`] variant that informs the handler about
/// the output of a successful upgrade on a new inbound substream.
///
/// Note that it is up to the [`ConnectionHandler`] implementation to manage the lifetime of the
/// negotiated inbound substreams. E.g. the implementation has to enforce a limit on the number
/// of simultaneously open negotiated inbound substreams. In other words it is up to the
/// [`ConnectionHandler`] implementation to stop a malicious remote node to open and keep alive
/// an excessive amount of inbound substreams.
pub struct FullyNegotiatedInbound<IP: InboundUpgradeSend, IOI> {
    pub protocol: IP::Output,
    pub info: IOI,
}

/// [`ConnectionEvent`] variant that informs the handler about successful upgrade on a new outbound stream.
///
/// The `protocol` field is the information that was previously passed to
/// [`ConnectionHandlerEvent::OutboundSubstreamRequest`].
pub struct FullyNegotiatedOutbound<OP: OutboundUpgradeSend, OOI> {
    pub protocol: OP::Output,
    pub info: OOI,
}

/// [`ConnectionEvent`] variant that informs the handler about a change in the address of the remote.
pub struct AddressChange<'a> {
    pub new_address: &'a Multiaddr,
}

/// [`ConnectionEvent`] variant that informs the handler about a change in the protocols supported on the connection.
#[derive(Clone)]
pub enum ProtocolsChange<'a> {
    Added(ProtocolsAdded<'a>),
    Removed(ProtocolsRemoved<'a>),
}

impl<'a> ProtocolsChange<'a> {
    /// Compute the [`ProtocolsChange`] that results from adding `to_add` to `existing_protocols`.
    ///
    /// Returns `None` if the change is a no-op, i.e. `to_add` is a subset of `existing_protocols`.
    pub(crate) fn add(
        existing_protocols: &'a HashSet<StreamProtocol>,
        to_add: &'a HashSet<StreamProtocol>,
    ) -> Option<Self> {
        let mut actually_added_protocols = to_add.difference(existing_protocols).peekable();

        actually_added_protocols.peek()?;

        Some(ProtocolsChange::Added(ProtocolsAdded {
            protocols: actually_added_protocols,
        }))
    }

    /// Compute the [`ProtocolsChange`] that results from removing `to_remove` from `existing_protocols`.
    ///
    /// Returns `None` if the change is a no-op, i.e. none of the protocols in `to_remove` are in `existing_protocols`.
    pub(crate) fn remove(
        existing_protocols: &'a HashSet<StreamProtocol>,
        to_remove: &'a HashSet<StreamProtocol>,
    ) -> Option<Self> {
        let mut actually_removed_protocols = existing_protocols.intersection(to_remove).peekable();

        actually_removed_protocols.peek()?;

        Some(ProtocolsChange::Removed(ProtocolsRemoved {
            protocols: Either::Right(actually_removed_protocols),
        }))
    }

    /// Compute the [`ProtocolsChange`]s required to go from `existing_protocols` to `new_protocols`.
    pub(crate) fn from_full_sets(
        existing_protocols: &'a HashSet<StreamProtocol>,
        new_protocols: &'a HashSet<StreamProtocol>,
    ) -> SmallVec<[Self; 2]> {
        if existing_protocols == new_protocols {
            return SmallVec::new();
        }

        let mut changes = SmallVec::new();

        let mut added_protocols = new_protocols.difference(existing_protocols).peekable();
        let mut removed_protocols = existing_protocols.difference(new_protocols).peekable();

        if added_protocols.peek().is_some() {
            changes.push(ProtocolsChange::Added(ProtocolsAdded {
                protocols: added_protocols,
            }));
        }

        if removed_protocols.peek().is_some() {
            changes.push(ProtocolsChange::Removed(ProtocolsRemoved {
                protocols: Either::Left(removed_protocols),
            }));
        }

        changes
    }
}

/// An [`Iterator`] over all protocols that have been added.
#[derive(Clone)]
pub struct ProtocolsAdded<'a> {
    protocols: Peekable<Difference<'a, StreamProtocol, RandomState>>,
}

impl<'a> ProtocolsAdded<'a> {
    pub(crate) fn from_set(protocols: &'a HashSet<StreamProtocol, RandomState>) -> Self {
        ProtocolsAdded {
            protocols: protocols.difference(&EMPTY_HASHSET).peekable(),
        }
    }
}

/// An [`Iterator`] over all protocols that have been removed.
#[derive(Clone)]
pub struct ProtocolsRemoved<'a> {
    protocols: Either<
        Peekable<Difference<'a, StreamProtocol, RandomState>>,
        Peekable<Intersection<'a, StreamProtocol, RandomState>>,
    >,
}

impl<'a> ProtocolsRemoved<'a> {
    #[cfg(test)]
    pub(crate) fn from_set(protocols: &'a HashSet<StreamProtocol, RandomState>) -> Self {
        ProtocolsRemoved {
            protocols: Either::Left(protocols.difference(&EMPTY_HASHSET).peekable()),
        }
    }
}

impl<'a> Iterator for ProtocolsAdded<'a> {
    type Item = &'a StreamProtocol;
    fn next(&mut self) -> Option<Self::Item> {
        self.protocols.next()
    }
}

impl<'a> Iterator for ProtocolsRemoved<'a> {
    type Item = &'a StreamProtocol;
    fn next(&mut self) -> Option<Self::Item> {
        self.protocols.next()
    }
}

/// [`ConnectionEvent`] variant that informs the handler
/// that upgrading an outbound substream to the given protocol has failed.
pub struct DialUpgradeError<OOI, OP: OutboundUpgradeSend> {
    pub info: OOI,
    pub error: StreamUpgradeError<OP::Error>,
}

/// [`ConnectionEvent`] variant that informs the handler
/// that upgrading an inbound substream to the given protocol has failed.
pub struct ListenUpgradeError<IOI, IP: InboundUpgradeSend> {
    pub info: IOI,
    pub error: IP::Error,
}

/// Configuration of inbound or outbound substream protocol(s)
/// for a [`ConnectionHandler`].
///
/// The inbound substream protocol(s) are defined by [`ConnectionHandler::listen_protocol`]
/// and the outbound substream protocol(s) by [`ConnectionHandlerEvent::OutboundSubstreamRequest`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SubstreamProtocol<TUpgrade, TInfo> {
    upgrade: TUpgrade,
    info: TInfo,
    timeout: Duration,
}

impl<TUpgrade, TInfo> SubstreamProtocol<TUpgrade, TInfo> {
    /// Create a new `SubstreamProtocol` from the given upgrade.
    ///
    /// The default timeout for applying the given upgrade on a substream is
    /// 10 seconds.
    pub fn new(upgrade: TUpgrade, info: TInfo) -> Self {
        SubstreamProtocol {
            upgrade,
            info,
            timeout: Duration::from_secs(10),
        }
    }

    /// Maps a function over the protocol upgrade.
    pub fn map_upgrade<U, F>(self, f: F) -> SubstreamProtocol<U, TInfo>
    where
        F: FnOnce(TUpgrade) -> U,
    {
        SubstreamProtocol {
            upgrade: f(self.upgrade),
            info: self.info,
            timeout: self.timeout,
        }
    }

    /// Maps a function over the protocol info.
    pub fn map_info<U, F>(self, f: F) -> SubstreamProtocol<TUpgrade, U>
    where
        F: FnOnce(TInfo) -> U,
    {
        SubstreamProtocol {
            upgrade: self.upgrade,
            info: f(self.info),
            timeout: self.timeout,
        }
    }

    /// Sets a new timeout for the protocol upgrade.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Borrows the contained protocol upgrade.
    pub fn upgrade(&self) -> &TUpgrade {
        &self.upgrade
    }

    /// Borrows the contained protocol info.
    pub fn info(&self) -> &TInfo {
        &self.info
    }

    /// Borrows the timeout for the protocol upgrade.
    pub fn timeout(&self) -> &Duration {
        &self.timeout
    }

    /// Converts the substream protocol configuration into the contained upgrade.
    pub fn into_upgrade(self) -> (TUpgrade, TInfo) {
        (self.upgrade, self.info)
    }
}

/// Event produced by a handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom, TErr> {
    /// Request a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest {
        /// The protocol(s) to apply on the substream.
        protocol: SubstreamProtocol<TConnectionUpgrade, TOutboundOpenInfo>,
    },

    /// Close the connection for the given reason.
    ///
    /// Note this will affect all [`ConnectionHandler`]s handling this
    /// connection, in other words it will close the connection for all
    /// [`ConnectionHandler`]s. To signal that one has no more need for the
    /// connection, while allowing other [`ConnectionHandler`]s to continue using
    /// the connection, return [`KeepAlive::No`] in
    /// [`ConnectionHandler::connection_keep_alive`].
    Close(TErr),
    /// We learned something about the protocols supported by the remote.
    ReportRemoteProtocols(ProtocolSupport),

    /// Event that is sent to a [`NetworkBehaviour`](crate::behaviour::NetworkBehaviour).
    NotifyBehaviour(TCustom),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolSupport {
    /// The remote now supports these additional protocols.
    Added(HashSet<StreamProtocol>),
    /// The remote no longer supports these protocols.
    Removed(HashSet<StreamProtocol>),
}

/// Event produced by a handler.
impl<TConnectionUpgrade, TOutboundOpenInfo, TCustom, TErr>
    ConnectionHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom, TErr>
{
    /// If this is an `OutboundSubstreamRequest`, maps the `info` member from a
    /// `TOutboundOpenInfo` to something else.
    pub fn map_outbound_open_info<F, I>(
        self,
        map: F,
    ) -> ConnectionHandlerEvent<TConnectionUpgrade, I, TCustom, TErr>
    where
        F: FnOnce(TOutboundOpenInfo) -> I,
    {
        match self {
            ConnectionHandlerEvent::OutboundSubstreamRequest { protocol } => {
                ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol.map_info(map),
                }
            }
            ConnectionHandlerEvent::NotifyBehaviour(val) => {
                ConnectionHandlerEvent::NotifyBehaviour(val)
            }
            ConnectionHandlerEvent::Close(val) => ConnectionHandlerEvent::Close(val),
            ConnectionHandlerEvent::ReportRemoteProtocols(support) => {
                ConnectionHandlerEvent::ReportRemoteProtocols(support)
            }
        }
    }

    /// If this is an `OutboundSubstreamRequest`, maps the protocol (`TConnectionUpgrade`)
    /// to something else.
    pub fn map_protocol<F, I>(
        self,
        map: F,
    ) -> ConnectionHandlerEvent<I, TOutboundOpenInfo, TCustom, TErr>
    where
        F: FnOnce(TConnectionUpgrade) -> I,
    {
        match self {
            ConnectionHandlerEvent::OutboundSubstreamRequest { protocol } => {
                ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol.map_upgrade(map),
                }
            }
            ConnectionHandlerEvent::NotifyBehaviour(val) => {
                ConnectionHandlerEvent::NotifyBehaviour(val)
            }
            ConnectionHandlerEvent::Close(val) => ConnectionHandlerEvent::Close(val),
            ConnectionHandlerEvent::ReportRemoteProtocols(support) => {
                ConnectionHandlerEvent::ReportRemoteProtocols(support)
            }
        }
    }

    /// If this is a `Custom` event, maps the content to something else.
    pub fn map_custom<F, I>(
        self,
        map: F,
    ) -> ConnectionHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, I, TErr>
    where
        F: FnOnce(TCustom) -> I,
    {
        match self {
            ConnectionHandlerEvent::OutboundSubstreamRequest { protocol } => {
                ConnectionHandlerEvent::OutboundSubstreamRequest { protocol }
            }
            ConnectionHandlerEvent::NotifyBehaviour(val) => {
                ConnectionHandlerEvent::NotifyBehaviour(map(val))
            }
            ConnectionHandlerEvent::Close(val) => ConnectionHandlerEvent::Close(val),
            ConnectionHandlerEvent::ReportRemoteProtocols(support) => {
                ConnectionHandlerEvent::ReportRemoteProtocols(support)
            }
        }
    }

    /// If this is a `Close` event, maps the content to something else.
    pub fn map_close<F, I>(
        self,
        map: F,
    ) -> ConnectionHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom, I>
    where
        F: FnOnce(TErr) -> I,
    {
        match self {
            ConnectionHandlerEvent::OutboundSubstreamRequest { protocol } => {
                ConnectionHandlerEvent::OutboundSubstreamRequest { protocol }
            }
            ConnectionHandlerEvent::NotifyBehaviour(val) => {
                ConnectionHandlerEvent::NotifyBehaviour(val)
            }
            ConnectionHandlerEvent::Close(val) => ConnectionHandlerEvent::Close(map(val)),
            ConnectionHandlerEvent::ReportRemoteProtocols(support) => {
                ConnectionHandlerEvent::ReportRemoteProtocols(support)
            }
        }
    }
}

#[deprecated(note = "Renamed to `StreamUpgradeError`")]
pub type ConnectionHandlerUpgrErr<TUpgrErr> = StreamUpgradeError<TUpgrErr>;

/// Error that can happen on an outbound substream opening attempt.
#[derive(Debug)]
pub enum StreamUpgradeError<TUpgrErr> {
    /// The opening attempt timed out before the negotiation was fully completed.
    Timeout,
    /// The upgrade produced an error.
    Apply(TUpgrErr),
    /// No protocol could be agreed upon.
    NegotiationFailed,
    /// An IO or otherwise unrecoverable error happened.
    Io(io::Error),
}

impl<TUpgrErr> StreamUpgradeError<TUpgrErr> {
    /// Map the inner [`StreamUpgradeError`] type.
    pub fn map_upgrade_err<F, E>(self, f: F) -> StreamUpgradeError<E>
    where
        F: FnOnce(TUpgrErr) -> E,
    {
        match self {
            StreamUpgradeError::Timeout => StreamUpgradeError::Timeout,
            StreamUpgradeError::Apply(e) => StreamUpgradeError::Apply(f(e)),
            StreamUpgradeError::NegotiationFailed => StreamUpgradeError::NegotiationFailed,
            StreamUpgradeError::Io(e) => StreamUpgradeError::Io(e),
        }
    }
}

impl<TUpgrErr> fmt::Display for StreamUpgradeError<TUpgrErr>
where
    TUpgrErr: error::Error + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamUpgradeError::Timeout => {
                write!(f, "Timeout error while opening a substream")
            }
            StreamUpgradeError::Apply(err) => {
                write!(f, "Apply: ")?;
                crate::print_error_chain(f, err)
            }
            StreamUpgradeError::NegotiationFailed => {
                write!(f, "no protocols could be agreed upon")
            }
            StreamUpgradeError::Io(e) => {
                write!(f, "IO error: ")?;
                crate::print_error_chain(f, e)
            }
        }
    }
}

impl<TUpgrErr> error::Error for StreamUpgradeError<TUpgrErr>
where
    TUpgrErr: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

/// How long the connection should be kept alive.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeepAlive {
    /// If nothing new happens, the connection should be closed at the given `Instant`.
    Until(Instant),
    /// Keep the connection alive.
    Yes,
    /// Close the connection as soon as possible.
    No,
}

impl KeepAlive {
    /// Returns true for `Yes`, false otherwise.
    pub fn is_yes(&self) -> bool {
        matches!(*self, KeepAlive::Yes)
    }
}

impl PartialOrd for KeepAlive {
    fn partial_cmp(&self, other: &KeepAlive) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for KeepAlive {
    fn cmp(&self, other: &KeepAlive) -> Ordering {
        use self::KeepAlive::*;

        match (self, other) {
            (No, No) | (Yes, Yes) => Ordering::Equal,
            (No, _) | (_, Yes) => Ordering::Less,
            (_, No) | (Yes, _) => Ordering::Greater,
            (Until(t1), Until(t2)) => t1.cmp(t2),
        }
    }
}

/// A statically declared, empty [`HashSet`] allows us to work around borrow-checker rules for
/// [`ProtocolsAdded::from_set`]. The lifetimes don't work unless we have a [`HashSet`] with a `'static' lifetime.
static EMPTY_HASHSET: Lazy<HashSet<StreamProtocol>> = Lazy::new(HashSet::new);
