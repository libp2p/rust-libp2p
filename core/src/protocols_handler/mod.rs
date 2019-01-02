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

use crate::upgrade::{
    InboundUpgrade,
    OutboundUpgrade,
    UpgradeError,
};
use futures::prelude::*;
use std::{error, fmt, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};

pub use self::dummy::DummyProtocolsHandler;
pub use self::map_in::MapInEvent;
pub use self::map_out::MapOutEvent;
pub use self::node_handler::{NodeHandlerWrapper, NodeHandlerWrapperBuilder};
pub use self::select::ProtocolsHandlerSelect;

mod dummy;
mod map_in;
mod map_out;
mod node_handler;
mod select;

/// Handler for a set of protocols for a specific connection with a remote.
///
/// This trait should be implemented on a struct that holds the state for a specific protocol
/// behaviour with a specific remote.
///
/// # Handling a protocol
///
/// Communication with a remote over a set of protocols opened in two different ways:
///
/// - Dialing, which is a voluntary process. In order to do so, make `poll()` return an
///   `OutboundSubstreamRequest` variant containing the connection upgrade to use to start using a protocol.
/// - Listening, which is used to determine which protocols are supported when the remote wants
///   to open a substream. The `listen_protocol()` method should return the upgrades supported when
///   listening.
///
/// The upgrade when dialing and the upgrade when listening have to be of the same type, but you
/// are free to return for example an `OrUpgrade` enum, or an enum of your own, containing the upgrade
/// you want depending on the situation.
///
/// # Shutting down
///
/// Implementors of this trait should keep in mind that the connection can be closed at any time.
/// When a connection is closed (either by us or by the remote) `shutdown()` is called and the
/// handler continues to be processed until it produces `None`. Only then the handler is destroyed.
///
/// This makes it possible for the handler to finish delivering events even after knowing that it
/// is shutting down.
///
/// Implementors of this trait should keep in mind that when `shutdown()` is called, the connection
/// might already be closed or unresponsive. They should therefore not rely on being able to
/// deliver messages.
///
/// # Relationship with `NodeHandler`.
///
/// This trait is very similar to the `NodeHandler` trait. The fundamental differences are:
///
/// - The `NodeHandler` trait gives you more control and is therefore more difficult to implement.
/// - The `NodeHandler` trait is designed to have exclusive ownership of the connection to a
///   node, while the `ProtocolsHandler` trait is designed to handle only a specific set of
///   protocols. Two or more implementations of `ProtocolsHandler` can be combined into one that
///   supports all the protocols together, which is not possible with `NodeHandler`.
///
// TODO: add a "blocks connection closing" system, so that we can gracefully close a connection
//       when it's no longer needed, and so that for example the periodic pinging system does not
//       keep the connection alive forever
pub trait ProtocolsHandler {
    /// Custom event that can be received from the outside.
    type InEvent;
    /// Custom event that can be produced by the handler and that will be returned to the outside.
    type OutEvent;
    /// Error that can happen when polling.
    type Error: error::Error;
    /// The type of the substream that contains the raw data.
    type Substream: AsyncRead + AsyncWrite;
    /// The upgrade for the protocol or protocols handled by this handler.
    type InboundProtocol: InboundUpgrade<Self::Substream>;
    /// The upgrade for the protocol or protocols handled by this handler.
    type OutboundProtocol: OutboundUpgrade<Self::Substream>;
    /// Information about a substream. Can be sent to the handler through a `NodeHandlerEndpoint`,
    /// and will be passed back in `inject_substream` or `inject_outbound_closed`.
    type OutboundOpenInfo;

    /// Produces a `ConnectionUpgrade` for the protocol or protocols to accept when listening.
    ///
    /// > **Note**: You should always accept all the protocols you support, even if in a specific
    /// >           context you wouldn't accept one in particular (eg. only allow one substream at
    /// >           a time for a given protocol). The reason is that remotes are allowed to put the
    /// >           list of supported protocols in a cache in order to avoid spurious queries.
    fn listen_protocol(&self) -> Self::InboundProtocol;

    /// Injects a fully-negotiated substream in the handler.
    ///
    /// This method is called when a substream has been successfully opened and negotiated.
    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output
    );

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        info: Self::OutboundOpenInfo
    );

    /// Injects an event coming from the outside in the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Indicates to the handler that upgrading a substream to the given protocol has failed.
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>);

    /// Indicates to the handler that the inbound part of the muxer has been closed, and that
    /// therefore no more inbound substreams will be produced.
    fn inject_inbound_closed(&mut self);

    /// Indicates to the node that it should shut down. After that, it is expected that `poll()`
    /// returns `Ready(None)` as soon as possible.
    ///
    /// This method allows an implementation to perform a graceful shutdown of the substreams, and
    /// send back various events.
    fn shutdown(&mut self);

    /// Should behave like `Stream::poll()`. Should close if no more event can be produced and the
    /// node should be closed.
    ///
    /// > **Note**: If this handler is combined with other handlers, as soon as `poll()` returns
    /// >           `Ok(Async::Ready(None))`, all the other handlers will receive a call to
    /// >           `shutdown()` and will eventually be closed and destroyed.
    fn poll(&mut self) -> Poll<Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>, Self::Error>;

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

    /// Builds an implementation of `ProtocolsHandler` that handles both this protocol and the
    /// other one together.
    #[inline]
    fn select<TProto2>(self, other: TProto2) -> ProtocolsHandlerSelect<Self, TProto2>
    where
        Self: Sized,
    {
        ProtocolsHandlerSelect::new(self, other)
    }

    /// Creates a builder that will allow creating a `NodeHandler` that handles this protocol
    /// exclusively.
    #[inline]
    fn into_node_handler_builder(self) -> NodeHandlerWrapperBuilder<Self>
    where
        Self: Sized,
    {
        NodeHandlerWrapperBuilder::new(self, Duration::from_secs(10), Duration::from_secs(10))
    }

    /// Builds an implementation of `NodeHandler` that handles this protocol exclusively.
    ///
    /// > **Note**: This is a shortcut for `self.into_node_handler_builder().build()`.
    #[inline]
    fn into_node_handler(self) -> NodeHandlerWrapper<Self>
    where
        Self: Sized,
    {
        self.into_node_handler_builder().build()
    }
}

/// Event produced by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom> {
    /// Request a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest {
        /// The upgrade to apply on the substream.
        upgrade: TConnectionUpgrade,
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
    /// If this is an `OutboundSubstreamRequest`, maps the `info` member from a `TOutboundOpenInfo` to something else.
    #[inline]
    pub fn map_outbound_open_info<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, I, TCustom>
    where
        F: FnOnce(TOutboundOpenInfo) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade,
                    info: map(info),
                }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
        }
    }

    /// If this is an `OutboundSubstreamRequest`, maps the protocol (`TConnectionUpgrade`) to something else.
    #[inline]
    pub fn map_protocol<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<I, TOutboundOpenInfo, TCustom>
    where
        F: FnOnce(TConnectionUpgrade) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade: map(upgrade),
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
            ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info }
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
    /// The remote muxer denied the attempt to open a substream.
    MuxerDeniedSubstream,
    /// Error while upgrading the substream to the protocol we want.
    Upgrade(UpgradeError<TUpgrErr>),
}

impl<TUpgrErr> fmt::Display for ProtocolsHandlerUpgrErr<TUpgrErr>
where
    TUpgrErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProtocolsHandlerUpgrErr::Timeout => {
                write!(f, "Timeout error while opening a substream")
            },
            ProtocolsHandlerUpgrErr::Timer => {
                write!(f, "Timer error while opening a substream")
            },
            ProtocolsHandlerUpgrErr::MuxerDeniedSubstream => {
                write!(f, "Remote muxer denied our attempt to open a substream")
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
            ProtocolsHandlerUpgrErr::MuxerDeniedSubstream => None,
            ProtocolsHandlerUpgrErr::Upgrade(err) => Some(err),
        }
    }
}
