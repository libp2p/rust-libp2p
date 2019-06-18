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

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io;

use futures::future::{self, FutureResult};
use libp2p_core::{upgrade::Negotiated, InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Copy, Clone)]
pub struct DeflateConfig;

/// Output of the deflate protocol.
pub type DeflateOutput<S> = DeflateDecoder<DeflateEncoder<S>>;

impl UpgradeInfo for DeflateConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/deflate/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for DeflateConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = DeflateOutput<Negotiated<C>>;
    type Error = io::Error;
    type Future = FutureResult<Self::Output, Self::Error>;

    fn upgrade_inbound(self, r: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ok(DeflateDecoder::new(DeflateEncoder::new(
            r,
            Compression::default(),
        )))
    }
}

impl<C> OutboundUpgrade<C> for DeflateConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = DeflateOutput<Negotiated<C>>;
    type Error = io::Error;
    type Future = FutureResult<Self::Output, Self::Error>;

    fn upgrade_outbound(self, w: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ok(DeflateDecoder::new(DeflateEncoder::new(
            w,
            Compression::default(),
        )))
    }
}
