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

use futures::prelude::*;
use futures::stream;
use multiaddr::Multiaddr;
use std::io::Error as IoError;
use transport::Transport;

/// Extension trait for `Transport`. Implemented on structs that provide a `Transport` on which
/// the dialed node can dial you back.
pub trait MuxedTransport: Transport {
    /// Future resolving to a future that will resolve to an incoming connection.
    type Incoming: Future<Item = Self::IncomingUpgrade, Error = IoError>;
    /// Future resolving to an incoming connection.
    type IncomingUpgrade: Future<Item = (Self::Output, Multiaddr), Error = IoError>;

    /// Returns the next incoming substream opened by a node that we dialed ourselves.
    ///
    /// > **Note**: Doesn't produce incoming substreams coming from addresses we are listening on.
    /// >            This only concerns nodes that we dialed with `dial()`.
    fn next_incoming(self) -> Self::Incoming
    where
        Self: Sized;

    /// Returns a stream of incoming connections.
    #[inline]
    fn incoming(
        self,
    ) -> stream::AndThen<stream::Repeat<Self, IoError>, fn(Self) -> Self::Incoming, Self::Incoming>
    where
        Self: Sized + Clone,
    {
        stream::repeat(self).and_then(|me| me.next_incoming())
    }
}
