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

use bytes::BytesMut;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use futures::{Future, StartSend, Poll, future};
use futures::sink::Sink;
use futures::stream::MapErr as StreamMapErr;
use futures::stream::Stream;
use libp2p_core::{identity, InboundUpgrade, OutboundUpgrade, UpgradeInfo, upgrade::Negotiated, PeerId, PublicKey};
use log::debug;
use rw_stream_sink::RwStreamSink;
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited::Framed;
use crate::error::PlainTextError;
use void::Void;
use futures::future::FutureResult;

mod error;
mod handshake;
mod pb;

#[derive(Debug, Copy, Clone)]
pub struct PlainText1Config;

impl UpgradeInfo for PlainText1Config {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/plaintext/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for PlainText1Config {
    type Output = Negotiated<C>;
    type Error = Void;
    type Future = FutureResult<Negotiated<C>, Self::Error>;

    fn upgrade_inbound(self, i: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ok(i)
    }
}

impl<C> OutboundUpgrade<C> for PlainText1Config {
    type Output = Negotiated<C>;
    type Error = Void;
    type Future = FutureResult<Negotiated<C>, Self::Error>;

    fn upgrade_outbound(self, i: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ok(i)
    }
}

#[derive(Clone)]
pub struct PlainText2Config {
    pub peer_id: PeerId,
    pub pubkey: identity::PublicKey,
}

impl UpgradeInfo for PlainText2Config {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/plaintext/2.0.0")
    }
}

impl<C> InboundUpgrade<C> for PlainText2Config
where
    C: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = PlainTextOutput<Negotiated<C>>;
    type Error = PlainTextError;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_inbound(self, socket: Negotiated<C>, _: Self::Info) -> Self::Future {
        Box::new(self.handshake(socket))
    }
}

impl<C> OutboundUpgrade<C> for PlainText2Config
where
    C: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = PlainTextOutput<Negotiated<C>>;
    type Error = PlainTextError;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_outbound(self, socket: Negotiated<C>, _: Self::Info) -> Self::Future {
        Box::new(self.handshake(socket))
    }
}

impl PlainText2Config {
    fn handshake<T>(self, socket: T) -> impl Future<Item = PlainTextOutput<T>, Error = PlainTextError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static
    {
        debug!("Starting plaintext upgrade");
        PlainTextMiddleware::handshake(socket, self)
            .map(|(stream_sink, public_key)| {
                let mapped = stream_sink.map_err(map_err as fn(_) -> _);
                PlainTextOutput {
                    stream: RwStreamSink::new(mapped),
                    remote_key: public_key,
                }
            })
    }
}

#[inline]
fn map_err(err: IoError) -> IoError {
    debug!("error during plaintext handshake {:?}", err);
    IoError::new(IoErrorKind::InvalidData, err)
}

pub struct PlainTextMiddleware<S> {
    pub inner: Framed<S, BytesMut>,
}

impl<S> PlainTextMiddleware<S>
where
    S: AsyncRead + AsyncWrite + Send,
{
    fn handshake(socket: S, config: PlainText2Config)
        -> impl Future<Item = (PlainTextMiddleware<S>, PublicKey), Error = PlainTextError>
    {
        handshake::handshake(socket, config).map(|(inner, public_key)| {
            (PlainTextMiddleware { inner }, public_key)
        })
    }
}

impl<S> Sink for PlainTextMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    type SinkItem = BytesMut;
    type SinkError = IoError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.inner.start_send(item)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.poll_complete()
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.close()
    }
}

impl<S> Stream for PlainTextMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    type Item = BytesMut;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}

/// Output of the plaintext protocol.
pub struct PlainTextOutput<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// The plaintext stream.
    pub stream: RwStreamSink<StreamMapErr<PlainTextMiddleware<S>, fn(IoError) -> IoError>>,
    /// The public key of the remote.
    pub remote_key: PublicKey,
}
