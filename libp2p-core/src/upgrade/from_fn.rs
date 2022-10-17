// Copyright 2020 Parity Technologies (UK) Ltd.
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

use crate::{
    upgrade::{InboundUpgrade, OutboundUpgrade, ProtocolName, UpgradeInfo},
    Endpoint,
};

use futures::prelude::*;
use std::iter;

/// Initializes a new [`FromFnUpgrade`].
///
/// # Example
///
/// ```
/// # use libp2p_core::transport::{Transport, MemoryTransport, memory::Channel};
/// # use libp2p_core::{upgrade, Negotiated};
/// # use std::io;
/// # use futures::AsyncWriteExt;
/// let _transport = MemoryTransport::default()
///     .and_then(move |out, cp| {
///         upgrade::apply(out, upgrade::from_fn("/foo/1", move |mut sock: Negotiated<Channel<Vec<u8>>>, endpoint| async move {
///             if endpoint.is_dialer() {
///                 upgrade::write_length_prefixed(&mut sock, "some handshake data").await?;
///                 sock.close().await?;
///             } else {
///                 let handshake_data = upgrade::read_length_prefixed(&mut sock, 1024).await?;
///                 if handshake_data != b"some handshake data" {
///                     return Err(io::Error::new(io::ErrorKind::Other, "bad handshake"));
///                 }
///             }
///             Ok(sock)
///         }), cp, upgrade::Version::V1)
///     });
/// ```
///
pub fn from_fn<P, F, C, Fut, Out, Err>(protocol_name: P, fun: F) -> FromFnUpgrade<P, F>
where
    // Note: these bounds are there in order to help the compiler infer types
    P: ProtocolName + Clone,
    F: FnOnce(C, Endpoint) -> Fut,
    Fut: Future<Output = Result<Out, Err>>,
{
    FromFnUpgrade { protocol_name, fun }
}

/// Implements the `UpgradeInfo`, `InboundUpgrade` and `OutboundUpgrade` traits.
///
/// The upgrade consists in calling the function passed when creating this struct.
#[derive(Debug, Clone)]
pub struct FromFnUpgrade<P, F> {
    protocol_name: P,
    fun: F,
}

impl<P, F> UpgradeInfo for FromFnUpgrade<P, F>
where
    P: ProtocolName + Clone,
{
    type Info = P;
    type InfoIter = iter::Once<P>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.protocol_name.clone())
    }
}

impl<C, P, F, Fut, Err, Out> InboundUpgrade<C> for FromFnUpgrade<P, F>
where
    P: ProtocolName + Clone,
    F: FnOnce(C, Endpoint) -> Fut,
    Fut: Future<Output = Result<Out, Err>>,
{
    type Output = Out;
    type Error = Err;
    type Future = Fut;

    fn upgrade_inbound(self, sock: C, _: Self::Info) -> Self::Future {
        (self.fun)(sock, Endpoint::Listener)
    }
}

impl<C, P, F, Fut, Err, Out> OutboundUpgrade<C> for FromFnUpgrade<P, F>
where
    P: ProtocolName + Clone,
    F: FnOnce(C, Endpoint) -> Fut,
    Fut: Future<Output = Result<Out, Err>>,
{
    type Output = Out;
    type Error = Err;
    type Future = Fut;

    fn upgrade_outbound(self, sock: C, _: Self::Info) -> Self::Future {
        (self.fun)(sock, Endpoint::Dialer)
    }
}
