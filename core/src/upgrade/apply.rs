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

use crate::ConnectedPoint;
use crate::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeError, ProtocolName};
use futures::prelude::*;

pub use multistream_select::Version;

/// Applies an upgrade to the inbound and outbound direction of a connection or substream.
pub async fn apply<C, U, O, E>(conn: C, up: U, cp: ConnectedPoint, v: Version)
    -> Result<O, UpgradeError<E>>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    U: InboundUpgrade<C, Output = O, Error = E> + OutboundUpgrade<C, Output = O, Error = E>,
{
    if cp.is_listener() {
        apply_inbound(conn, up).await
    } else {
        apply_outbound(conn, up, v).await
    }
}

/// Tries to perform an upgrade on an inbound connection or substream.
pub async fn apply_inbound<C, U>(conn: C, up: U) -> Result<U::Output, UpgradeError<U::Error>>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    U: InboundUpgrade<C>,
{
    let iter = up.protocol_info().into_iter().map(NameWrap as fn(_) -> NameWrap<_>);
    let (info, io) = multistream_select::listener_select_proto(conn, iter).await?;
    Ok(up.upgrade_inbound(io, info.0).await.map_err(UpgradeError::Apply)?)
}

/// Tries to perform an upgrade on an outbound connection or substream.
pub async fn apply_outbound<C, U>(conn: C, up: U, v: Version)
    -> Result<U::Output, UpgradeError<U::Error>>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    U: OutboundUpgrade<C>
{
    let iter = up.protocol_info().into_iter().map(NameWrap as fn(_) -> NameWrap<_>);
    let (info, io) = multistream_select::dialer_select_proto(conn, iter, v).await?;
    Ok(up.upgrade_outbound(io, info.0).await.map_err(UpgradeError::Apply)?)
}

/// Wrapper type to expose an `AsRef<[u8]>` impl for all types implementing `ProtocolName`.
#[derive(Clone)]
struct NameWrap<N>(N);

impl<N: ProtocolName> AsRef<[u8]> for NameWrap<N> {
    fn as_ref(&self) -> &[u8] {
        self.0.protocol_name()
    }
}
