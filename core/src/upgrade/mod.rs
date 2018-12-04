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

//! Contains everything related to upgrading a connection or a substream to use a protocol.
//!
//! After a connection with a remote has been successfully established or a substream successfully
//! opened, the next step is to *upgrade* this connection or substream to use a protocol.
//!
//! This is where the `UpgradeInfo`, `InboundUpgrade` and `OutboundUpgrade` traits come into play.
//! The `InboundUpgrade` and `OutboundUpgrade` traits are implemented on types that represent a
//! collection of one or more possible protocols for respectively an ingoing or outgoing
//! connection or substream.
//!
//! > **Note**: Multiple versions of the same protocol are treated as different protocols.
//! >           For example, `/foo/1.0.0` and `/foo/1.1.0` are totally unrelated as far as
//! >           upgrading is concerned.
//!
//! # Upgrade process
//!
//! An upgrade is performed in two steps:
//!
//! - A protocol negotiation step. The `UpgradeInfo::protocol_names` method is called to determine
//!   which protocols are supported by the trait implementation. The `multistream-select` protocol
//!   is used in order to agree on which protocol to use amongst the ones supported.
//!
//! - A handshake. After a successful negotiation, the `InboundUpgrade::upgrade_inbound` or
//!   `OutboundUpgrade::upgrade_outbound` method is called. This method will return a `Future` that
//!   performs a handshake. This handshake is considered mandatory, however in practice it is
//!   possible for the trait implementation to return a dummy `Future` that doesn't perform any
//!   action and immediately succeeds.
//!
//! After an upgrade is successful, an object of type `InboundUpgrade::Output` or
//! `OutboundUpgrade::Output` is returned. The actual object depends on the implementation and
//! there is no constraint on the traits that it should implement, however it is expected that it
//! can be used by the user to control the behaviour of the protocol.
//!
//! > **Note**: You can use the `apply_inbound` or `apply_outbound` methods to try upgrade a
//!             connection or substream. However if you use the recommended `Swarm` or
//!             `ProtocolsHandler` APIs, the upgrade is automatically handled for you and you don't
//!             need to use these methods.
//!

mod apply;
mod denied;
mod either;
mod error;
mod map;
mod or;

use bytes::Bytes;
use crate::Endpoint;
use futures::future::Future;

pub use self::{
    apply::{apply, apply_inbound, apply_outbound, InboundUpgradeApply, OutboundUpgradeApply},
    denied::DeniedUpgrade,
    either::EitherUpgrade,
    error::UpgradeError,
    map::{Map, MapErr},
    or::Or
};

/// Trait for upgrades that can be applied to substreams.
pub trait Upgrade<T> {
    /// Opaque type representing a negotiable protocol.
    type UpgradeId;
    /// Iterator returned by `protocol_names`.
    type NamesIter: Iterator<Item = (Bytes, Self::UpgradeId)>;
    /// Output after the upgrade has been successfully negotiated and the handshake performed.
    type Output;
    /// Possible error during the handshake.
    type Error;
    /// Future that performs the handshake with the remote.
    type Future: Future<Item = Self::Output, Error = Self::Error>;

    /// Returns the list of protocols that are supported. Used during the negotiation process.
    ///
    /// Each item returned by the iterator is a pair of a protocol name and an opaque identifier.
    fn protocol_names(&self) -> Self::NamesIter;

    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start the handshake.
    ///
    /// The `id` is the identifier of the protocol, as produced by `protocol_names()`.
    fn upgrade(self, input: T, id: Self::UpgradeId, end: Endpoint) -> Self::Future;
}

/// Extension trait for `Upgrade`.
///
/// Automatically implemented on all types that implement `Upgrade`.
pub trait UpgradeExt<T>: Upgrade<T> {
    /// Returns a new object that wraps around `Self` and applies a closure to the `Output`.
    fn map<F, A>(self, f: F) -> Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output) -> A
    {
        map::map(self, f)
    }

    /// Returns a new object that wraps around `Self` and applies a closure to the `Error`.
    fn map_err<F, A>(self, f: F) -> MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error) -> A
    {
        map::map_err(self, f)
    }

    /// Returns a new object that combines `Self` and another upgrade to support both at the same
    /// time.
    fn or<U>(self, other: U) -> Or<Self, U>
    where
        Self: Sized,
        U: Upgrade<T>
    {
        or::or(self, other)
    }
}

impl<T, U: Upgrade<T>> UpgradeExt<T> for U {}

