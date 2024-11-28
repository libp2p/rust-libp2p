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

//! Implementation of the [plaintext](https://github.com/libp2p/specs/blob/master/plaintext/README.md) protocol.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use std::{
    io, iter,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{
    upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade},
    UpgradeInfo,
};
use libp2p_identity as identity;
use libp2p_identity::{PeerId, PublicKey};

use crate::error::Error;

mod error;
mod handshake;
mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub(crate) use self::structs::Exchange;
}

/// [`Config`] is an insecure connection handshake for testing purposes only.
#[derive(Clone)]
pub struct Config {
    local_public_key: identity::PublicKey,
}

impl Config {
    pub fn new(identity: &identity::Keypair) -> Self {
        Self {
            local_public_key: identity.public(),
        }
    }
}

impl UpgradeInfo for Config {
    type Info = &'static str;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once("/plaintext/2.0.0")
    }
}

impl<C> InboundConnectionUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = (PeerId, Output<C>);
    type Error = Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        Box::pin(self.handshake(socket))
    }
}

impl<C> OutboundConnectionUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = (PeerId, Output<C>);
    type Error = Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        Box::pin(self.handshake(socket))
    }
}

impl Config {
    async fn handshake<T>(self, socket: T) -> Result<(PeerId, Output<T>), Error>
    where
        T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        tracing::debug!("Starting plaintext handshake.");
        let (socket, remote_key, read_buffer) = handshake::handshake(socket, self).await?;
        tracing::debug!("Finished plaintext handshake.");

        Ok((
            remote_key.to_peer_id(),
            Output {
                socket,
                remote_key,
                read_buffer,
            },
        ))
    }
}

/// Output of the plaintext protocol.
pub struct Output<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// The plaintext stream.
    pub socket: S,
    /// The public key of the remote.
    pub remote_key: PublicKey,
    /// Remaining bytes that have been already buffered
    /// during the handshake but are not part of the
    /// handshake. These must be consumed first by `poll_read`.
    read_buffer: Bytes,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for Output<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        if !self.read_buffer.is_empty() {
            let n = std::cmp::min(buf.len(), self.read_buffer.len());
            let b = self.read_buffer.split_to(n);
            buf[..n].copy_from_slice(&b[..]);
            return Poll::Ready(Ok(n));
        }
        AsyncRead::poll_read(Pin::new(&mut self.socket), cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for Output<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        AsyncWrite::poll_write(Pin::new(&mut self.socket), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.socket), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        AsyncWrite::poll_close(Pin::new(&mut self.socket), cx)
    }
}
