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
use futures::{Future, StartSend, Poll, future};
use futures::sink::Sink;
use futures::stream::MapErr as StreamMapErr;
use futures::stream::Stream;
use libp2p_core::{identity, InboundUpgrade, OutboundUpgrade, UpgradeInfo, upgrade::Negotiated, PeerId, PublicKey};
use log::debug;
use rw_stream_sink::RwStreamSink;
use std::io;
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited::Framed;
use crate::error::PlainTextError;
use void::Void;
use futures::future::FutureResult;
use crate::handshake::Remote;

mod error;
mod handshake;
mod pb;

/// `PlainText1Config` is an insecure connection handshake for testing purposes only.
///
/// > **Note**: Given that `PlainText1Config` has no notion of exchanging peer identity information it is not compatible
/// > with the `libp2p_core::transport::upgrade::Builder` pattern. See
/// > [`PlainText2Config`](struct.PlainText2Config.html) if compatibility is needed. Even though not compatible with the
/// > Builder pattern one can still do an upgrade *manually*:
///
/// ```
/// # use libp2p_core::transport::{ Transport, memory::MemoryTransport };
/// # use libp2p_plaintext::PlainText1Config;
/// #
/// MemoryTransport::default()
///   .and_then(move |io, endpoint| {
///     libp2p_core::upgrade::apply(
///       io,
///       PlainText1Config{},
///       endpoint,
///       libp2p_core::transport::upgrade::Version::V1,
///     )
///   })
///   .map(|plaintext, _endpoint| {
///     unimplemented!();
///     // let peer_id = somehow_derive_peer_id();
///     // return (peer_id, plaintext);
///   });
/// ```
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

/// `PlainText2Config` is an insecure connection handshake for testing purposes only, implementing
/// the libp2p plaintext connection handshake specification.
#[derive(Clone)]
pub struct PlainText2Config {
    pub local_public_key: identity::PublicKey,
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
    type Output = (PeerId, PlainTextOutput<Negotiated<C>>);
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
    type Output = (PeerId, PlainTextOutput<Negotiated<C>>);
    type Error = PlainTextError;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_outbound(self, socket: Negotiated<C>, _: Self::Info) -> Self::Future {
        Box::new(self.handshake(socket))
    }
}

impl PlainText2Config {
    fn handshake<T>(self, socket: T) -> impl Future<Item = (PeerId, PlainTextOutput<T>), Error = PlainTextError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static
    {
        debug!("Starting plaintext upgrade");
        PlainTextMiddleware::handshake(socket, self)
            .map(|(stream_sink, remote)| {
                let mapped = stream_sink.map_err(map_err as fn(_) -> _);
                (
                    remote.peer_id,
                    PlainTextOutput {
                        stream: RwStreamSink::new(mapped),
                        remote_key: remote.public_key,
                    }
                )
            })
    }
}

#[inline]
fn map_err(err: io::Error) -> io::Error {
    debug!("error during plaintext handshake {:?}", err);
    io::Error::new(io::ErrorKind::InvalidData, err)
}

pub struct PlainTextMiddleware<S> {
    inner: Framed<S, BytesMut>,
}

impl<S> PlainTextMiddleware<S>
where
    S: AsyncRead + AsyncWrite + Send,
{
    fn handshake(socket: S, config: PlainText2Config)
        -> impl Future<Item = (PlainTextMiddleware<S>, Remote), Error = PlainTextError>
    {
        handshake::handshake(socket, config).map(|(inner, remote)| {
            (PlainTextMiddleware { inner }, remote)
        })
    }
}

impl<S> Sink for PlainTextMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    type SinkItem = BytesMut;
    type SinkError = io::Error;

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
    type Error = io::Error;

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
    pub stream: RwStreamSink<StreamMapErr<PlainTextMiddleware<S>, fn(io::Error) -> io::Error>>,
    /// The public key of the remote.
    pub remote_key: PublicKey,
}

impl<S: AsyncRead + AsyncWrite> std::io::Read for PlainTextOutput<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stream.read(buf)
    }
}

impl<S: AsyncRead + AsyncWrite> AsyncRead for PlainTextOutput<S> {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.stream.prepare_uninitialized_buffer(buf)
    }
}

impl<S: AsyncRead + AsyncWrite> std::io::Write for PlainTextOutput<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

impl<S: AsyncRead + AsyncWrite> AsyncWrite for PlainTextOutput<S> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.stream.shutdown()
    }
}
