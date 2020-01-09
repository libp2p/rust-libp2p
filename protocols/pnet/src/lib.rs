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

use futures::future::{self};
use futures::prelude::*;
use futures::future::BoxFuture;
use libp2p_core::{
    InboundUpgrade,
    OutboundUpgrade,
    UpgradeInfo,
    upgrade::Negotiated,
};
use std::{io, iter, pin::Pin, task::{Context, Poll}};
use void::Void;

#[derive(Debug, Copy, Clone)]
pub struct PnetConfig;

impl UpgradeInfo for PnetConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/pnet/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for PnetConfig
    where
        C: AsyncRead + AsyncWrite + Send + Unpin + 'static {
    type Output = PnetOutput<Negotiated<C>>;
    type Error = Void;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, i: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(PnetOutput::new(i))).boxed()
    }
}

impl<C> OutboundUpgrade<C> for PnetConfig
    where
        C: AsyncRead + AsyncWrite + Send + Unpin + 'static {
    type Output = PnetOutput<Negotiated<C>>;
    type Error = Void;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, i: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(PnetOutput::new(i))).boxed()
    }
}

/// Output of the pnet protocol.
pub struct PnetOutput<S>
{
    inner: S,
}

impl<S: AsyncRead + AsyncWrite + Unpin> PnetOutput<S> {
    fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for PnetOutput<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8])
        -> Poll<Result<usize, io::Error>>
    {
        Pin::new(&mut self.as_mut().inner).poll_read(cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for PnetOutput<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8])
        -> Poll<Result<usize, io::Error>>
    {
        Pin::new(&mut self.as_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context)
        -> Poll<Result<(), io::Error>>
    {
        Pin::new(&mut self.as_mut().inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context)
        -> Poll<Result<(), io::Error>>
    {
        Pin::new(&mut self.as_mut().inner).poll_close(cx)
    }
}
