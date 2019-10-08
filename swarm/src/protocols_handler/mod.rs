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

//! Once a connection to a remote peer is established, a `ProtocolsHandler` negotiates
//! and handles one or more specific protocols on the connection.
//!
//! Protocols are negotiated and used on individual substreams of the connection.
//! Thus a `ProtocolsHandler` defines the inbound and outbound upgrades to apply
//! when creating a new inbound or outbound substream, respectively, and is notified
//! by a `Swarm` when these upgrades have been successfully applied, including the
//! final output of the upgrade. A `ProtocolsHandler` can then continue communicating
//! with the peer over the substream using the negotiated protocol(s).
//!
//! Two `ProtocolsHandler`s can be composed with [`ProtocolsHandler::select()`]
//! in order to build a new handler supporting the combined set of protocols,
//! with methods being dispatched to the appropriate handler according to the
//! used protocol(s) determined by the associated types of the handlers.
//!
//! > **Note**: A `ProtocolsHandler` handles one or more protocols in the context of a single
//! >           connection with a remote. In order to handle a protocol that requires knowledge of
//! >           the network as a whole, see the `NetworkBehaviour` trait.

mod dummy;
mod map_in;
mod map_out;
mod node_handler;
mod one_shot;
mod select;

use futures::prelude::*;
use libp2p_core::{
    ConnectedPoint,
    PeerId,
    upgrade::{self, InboundUpgrade, OutboundUpgrade, UpgradeError},
};
use std::{cmp::Ordering, error, fmt, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::Instant;

pub use dummy::DummyProtocolsHandler;
pub use map_in::MapInEvent;
pub use map_out::MapOutEvent;
pub use node_handler::{NodeHandlerWrapper, NodeHandlerWrapperBuilder, NodeHandlerWrapperError};
pub use one_shot::OneShotHandler;
pub use select::{IntoProtocolsHandlerSelect, ProtocolsHandlerSelect};

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
///      [`ProtocolsHandler::poll()`] must return an [`OutboundSubstreamRequest`], providing an
///      instance of [`ProtocolsHandler::OutboundUpgrade`] that is used to negotiate the
///      protocol(s). Upon success, [`ProtocolsHandler::inject_fully_negotiated_outbound`]
///      is called with the final output of the upgrade.
///
///   2. Listening by accepting a new inbound substream. When a new inbound substream
///      is created on a connection, [`ProtocolsHandler::listen_protocol`] is called
///      to obtain an instance of [`ProtocolsHandler::InboundUpgrade`] that is used to
///      negotiate the protocol(s). Upon success,
///      [`ProtocolsHandler::inject_fully_negotiated_inbound`] is called with the final
///      output of the upgrade.
///
/// # Connection Keep-Alive
///
/// A `ProtocolsHandler` can influence the lifetime of the underlying connection
/// through [`ProtocolsHandler::connection_keep_alive`]. That is, the protocol
/// implemented by the handler can include conditions for terminating the connection.
/// The lifetime of successfully negotiated substreams is fully controlled by the handler.
///
/// Implementors of this trait should keep in mind that the connection can be closed at any time.
/// When a connection is closed gracefully, the substreams used by the handler may still
/// continue reading data until the remote closes its side of the connection.
pub trait ProtocolsHandler {
    /// Custom event that can be received from the outside.
    type InEvent;
    /// Custom event that can be produced by the handler and that will be returned to the outside.
    type OutEvent;
    /// The type of errors returned by [`ProtocolsHandler::poll`].
    type Error: error::Error;
    /// The type of substreams on which the protocol(s) are negotiated.
    type Substream: AsyncRead + AsyncWrite;
    /// The inbound upgrade for the protocol(s) used by the handler.
    type InboundProtocol: InboundUpgrade<Self::Substream>;
    /// The outbound upgrade for the protocol(s) used by the handler.
    type OutboundProtocol: OutboundUpgrade<Self::Substream>;
    /// The type of additional information passed to an `OutboundSubstreamRequest`.
    type OutboundOpenInfo;

    /// The [`InboundUpgrade`] to apply on inbound substreams to negotiate the
    /// desired protocols.
    ///
    /// > **Note**: The returned `InboundUpgrade` should always accept all the generally
    /// >           supported protocols, even if in a specific context a particular one is
    /// >           not supported, (eg. when only allowing one substream at a time for a protocol).
    /// >           This allows a remote to put the list of supported protocols in a cache.
    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol>;

    /// Injects the output of a successful upgrade on a new inbound substream.
    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output
    );

    /// Injects the output of a successful upgrade on a new outbound substream.
    ///
    /// The second argument is the information that was previously passed to
    /// [`ProtocolsHandlerEvent::OutboundSubstreamRequest`].
    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        info: Self::OutboundOpenInfo
    );

    /// Injects an event coming from the outside in the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Indicates to the handler that upgrading a substream to the given protocol has failed.
    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error
        >
    );

    /// Returns until when the connection should be kept alive.
    ///
    /// This method is called by the `Swarm` after each invocation of
    /// [`ProtocolsHandler::poll`] to determine if the connection and the associated
    /// `ProtocolsHandler`s should be kept alive as far as this handler is concerned
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
    /// > when [`ProtocolsHandler::poll`] returns an error. Furthermore, the
    /// > connection may be closed for reasons outside of the control
    /// > of the handler.
    fn connection_keep_alive(&self) -> KeepAlive;

    /// Should behave like `Stream::poll()`.
    ///
    /// Returning an error will close the connection to the remote.
    fn poll(&mut self) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        Self::Error
    >;

    /// Adds a closure that turns the input event into something else.
    #[inline]
    fn map_in_event<TNewIn, TMap>(self, map: TMap) -> MapInEvent<Self, TNewIn, TMap>
    where
        Self: Sized,
        TMap: Fn(&TNewIn) -> Option<&Self::InEvent>,
    {
        MapInEvent::new(self, map)
    }

    /// Adds a closure that turns the output event into something else.
    #[inline]
    fn map_out_event<TMap, TNewOut>(self, map: TMap) -> MapOutEvent<Self, TMap>
    where
        Self: Sized,
        TMap: FnMut(Self::OutEvent) -> TNewOut,
    {
        MapOutEvent::new(self, map)
    }

    /// Creates a new `ProtocolsHandler` that selects either this handler or
    /// `other` by delegating methods calls appropriately.
    ///
    /// > **Note**: The largest `KeepAlive` returned by the two handlers takes precedence,
    /// > i.e. is returned from [`ProtocolsHandler::connection_keep_alive`] by the returned
    /// > handler.
    #[inline]
    fn select<TProto2>(self, other: TProto2) -> ProtocolsHandlerSelect<Self, TProto2>
    where
        Self: Sized,
    {
        ProtocolsHandlerSelect::new(self, other)
    }

    /// Creates a builder that allows creating a `NodeHandler` that handles this protocol
    /// exclusively.
    ///
    /// > **Note**: This method should not be redefined in a custom `ProtocolsHandler`.
    #[inline]
    fn into_node_handler_builder(self) -> NodeHandlerWrapperBuilder<Self>
    where
        Self: Sized,
    {
        IntoProtocolsHandler::into_node_handler_builder(self)
    }

    /// Builds an implementation of `NodeHandler` that handles this protocol exclusively.
    ///
    /// > **Note**: This is a shortcut for `self.into_node_handler_builder().build()`.
    #[inline]
    #[deprecated(note = "Use into_node_handler_builder instead")]
    fn into_node_handler(self) -> NodeHandlerWrapper<Self>
    where
        Self: Sized,
    {
        #![allow(deprecated)]
        self.into_node_handler_builder().build()
    }
}

/// Configuration of inbound or outbound substream protocol(s)
/// for a [`ProtocolsHandler`].
///
/// The inbound substream protocol(s) are defined by [`ProtocolsHandler::listen_protocol`]
/// and the outbound substream protocol(s) by [`ProtocolsHandlerEvent::OutboundSubstreamRequest`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SubstreamProtocol<TUpgrade> {
    upgrade: TUpgrade,
    upgrade_protocol: upgrade::Version,
    timeout: Duration,
}

impl<TUpgrade> SubstreamProtocol<TUpgrade> {
    /// Create a new `ListenProtocol` from the given upgrade.
    ///
    /// The default timeout for applying the given upgrade on a substream is
    /// 10 seconds.
    pub fn new(upgrade: TUpgrade) -> SubstreamProtocol<TUpgrade> {
        SubstreamProtocol {
            upgrade,
            upgrade_protocol: upgrade::Version::V1,
            timeout: Duration::from_secs(10),
        }
    }

    /// Sets the multistream-select protocol (version) to use for negotiating
    /// protocols upgrades on outbound substreams.
    pub fn with_upgrade_protocol(mut self, version: upgrade::Version) -> Self {
        self.upgrade_protocol = version;
        self
    }

    /// Maps a function over the protocol upgrade.
    pub fn map_upgrade<U, F>(self, f: F) -> SubstreamProtocol<U>
    where
        F: FnOnce(TUpgrade) -> U,
    {
        SubstreamProtocol {
            upgrade: f(self.upgrade),
            upgrade_protocol: self.upgrade_protocol,
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

    /// Borrows the timeout for the protocol upgrade.
    pub fn timeout(&self) -> &Duration {
        &self.timeout
    }

    /// Converts the substream protocol configuration into the contained upgrade.
    pub fn into_upgrade(self) -> (upgrade::Version, TUpgrade) {
        (self.upgrade_protocol, self.upgrade)
    }
}

impl<TUpgrade> From<TUpgrade> for SubstreamProtocol<TUpgrade> {
    fn from(upgrade: TUpgrade) -> SubstreamProtocol<TUpgrade> {
        SubstreamProtocol::new(upgrade)
    }
}

/// Event produced by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom> {
    /// Request a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest {
        /// The protocol(s) to apply on the substream.
        protocol: SubstreamProtocol<TConnectionUpgrade>,
        /// User-defined information, passed back when the substream is open.
        info: TOutboundOpenInfo,
    },

    /// Other event.
    Custom(TCustom),
}

/// Event produced by a handler.
impl<TConnectionUpgrade, TOutboundOpenInfo, TCustom>
    ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom>
{
    /// If this is an `OutboundSubstreamRequest`, maps the `info` member from a
    /// `TOutboundOpenInfo` to something else.
    #[inline]
    pub fn map_outbound_open_info<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, I, TCustom>
    where
        F: FnOnce(TOutboundOpenInfo) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol,
                    info: map(info),
                }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
        }
    }

    /// If this is an `OutboundSubstreamRequest`, maps the protocol (`TConnectionUpgrade`)
    /// to something else.
    #[inline]
    pub fn map_protocol<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<I, TOutboundOpenInfo, TCustom>
    where
        F: FnOnce(TConnectionUpgrade) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: protocol.map_upgrade(map),
                    info,
                }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
        }
    }

    /// If this is a `Custom` event, maps the content to something else.
    #[inline]
    pub fn map_custom<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, I>
    where
        F: FnOnce(TCustom) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(map(val)),
        }
    }
}

/// Error that can happen on an outbound substream opening attempt.
#[derive(Debug)]
pub enum ProtocolsHandlerUpgrErr<TUpgrErr> {
    /// The opening attempt timed out before the negotiation was fully completed.
    Timeout,
    /// There was an error in the timer used.
    Timer,
    /// Error while upgrading the substream to the protocol we want.
    Upgrade(UpgradeError<TUpgrErr>),
}

impl<TUpgrErr> fmt::Display for ProtocolsHandlerUpgrErr<TUpgrErr>
where
    TUpgrErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolsHandlerUpgrErr::Timeout => {
                write!(f, "Timeout error while opening a substream")
            },
            ProtocolsHandlerUpgrErr::Timer => {
                write!(f, "Timer error while opening a substream")
            },
            ProtocolsHandlerUpgrErr::Upgrade(err) => write!(f, "{}", err),
        }
    }
}

impl<TUpgrErr> error::Error for ProtocolsHandlerUpgrErr<TUpgrErr>
where
    TUpgrErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            ProtocolsHandlerUpgrErr::Timeout => None,
            ProtocolsHandlerUpgrErr::Timer => None,
            ProtocolsHandlerUpgrErr::Upgrade(err) => Some(err),
        }
    }
}

/// Prototype for a `ProtocolsHandler`.
pub trait IntoProtocolsHandler {
    /// The protocols handler.
    type Handler: ProtocolsHandler;

    /// Builds the protocols handler.
    ///
    /// The `PeerId` is the id of the node the handler is going to handle.
    fn into_handler(self, remote_peer_id: &PeerId, connected_point: &ConnectedPoint) -> Self::Handler;

    /// Return the handler's inbound protocol.
    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol;

    /// Builds an implementation of `IntoProtocolsHandler` that handles both this protocol and the
    /// other one together.
    fn select<TProto2>(self, other: TProto2) -> IntoProtocolsHandlerSelect<Self, TProto2>
    where
        Self: Sized,
    {
        IntoProtocolsHandlerSelect::new(self, other)
    }

    /// Creates a builder that will allow creating a `NodeHandler` that handles this protocol
    /// exclusively.
    fn into_node_handler_builder(self) -> NodeHandlerWrapperBuilder<Self>
    where
        Self: Sized,
    {
        NodeHandlerWrapperBuilder::new(self)
    }
}

impl<T> IntoProtocolsHandler for T
where T: ProtocolsHandler
{
    type Handler = Self;

    fn into_handler(self, _: &PeerId, _: &ConnectedPoint) -> Self {
        self
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        self.listen_protocol().into_upgrade().1
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
        match *self {
            KeepAlive::Yes => true,
            _ => false,
        }
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
            (No, No) | (Yes, Yes)  => Ordering::Equal,
            (No,  _) | (_,   Yes)  => Ordering::Less,
            (_,  No) | (Yes,   _)  => Ordering::Greater,
            (Until(t1), Until(t2)) => t1.cmp(t2),
        }
    }
}
