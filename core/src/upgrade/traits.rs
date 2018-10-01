// Copyright 2017 Parity Technologies (UK) Ltd.
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

use bytes::Bytes;
use futures::future::Future;
use std::{io::Error as IoError, ops::Not};

/// Type of connection for the upgrade.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// The socket comes from a dialer.
    Dialer,
    /// The socket comes from a listener.
    Listener,
}

impl Not for Endpoint {
    type Output = Endpoint;

    fn not(self) -> Self::Output {
        match self {
            Endpoint::Dialer => Endpoint::Listener,
            Endpoint::Listener => Endpoint::Dialer
        }
    }
}


/// Implemented on structs that describe a possible upgrade to a connection between two peers.
///
/// The generic `C` is the type of the incoming connection before it is upgraded.
///
/// > **Note**: The `upgrade` method of this trait uses `self` and not `&self` or `&mut self`.
/// >           This has been designed so that you would implement this trait on `&Foo` or
/// >           `&mut Foo` instead of directly on `Foo`.
pub trait ConnectionUpgrade<C> {
    /// Iterator returned by `protocol_names`.
    type NamesIter: Iterator<Item = (Bytes, Self::UpgradeIdentifier)>;
    /// Type that serves as an identifier for the protocol. This type only exists to be returned
    /// by the `NamesIter` and then be passed to `upgrade`.
    ///
    /// This is only useful on implementations that dispatch between multiple possible upgrades.
    /// Any basic implementation will probably just use the `()` type.
    type UpgradeIdentifier;

    /// Returns the name of the protocols to advertise to the remote.
    fn protocol_names(&self) -> Self::NamesIter;

    /// Type of the stream that has been upgraded. Generally wraps around `C` and `Self`.
    ///
    /// > **Note**: For upgrades that add an intermediary layer (such as `secio` or `multiplex`),
    /// >           this associated type must implement `AsyncRead + AsyncWrite`.
    type Output;
    /// Type of the future that will resolve to `Self::Output`.
    type Future: Future<Item = Self::Output, Error = IoError>;

    /// This method is called after protocol negotiation has been performed.
    ///
    /// Because performing the upgrade may not be instantaneous (eg. it may require a handshake),
    /// this function returns a future instead of the direct output.
    fn upgrade(
        self,
        socket: C,
        id: Self::UpgradeIdentifier,
        ty: Endpoint,
    ) -> Self::Future;
}
