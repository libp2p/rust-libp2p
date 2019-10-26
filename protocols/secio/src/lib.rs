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

//! The `secio` protocol is a middleware that will encrypt and decrypt communications going
//! through a socket (or anything that implements `AsyncRead + AsyncWrite`).
//!
//! # Usage
//!
//! The `SecioConfig` implements [`InboundUpgrade`] and [`OutboundUpgrade`] and thus
//! serves as a connection upgrade for authentication of a transport.
//! See [`authenticate`](libp2p_core::transport::upgrade::builder::Builder::authenticate).
//!
//! ```no_run
//! # fn main() {
//! use futures::prelude::*;
//! use libp2p_secio::{SecioConfig, SecioOutput};
//! use libp2p_core::{PeerId, Multiaddr, identity, upgrade};
//! use libp2p_core::transport::Transport;
//! use libp2p_mplex::MplexConfig;
//! use libp2p_tcp::TcpConfig;
//!
//! // Create a local peer identity.
//! let local_keys = identity::Keypair::generate_ed25519();
//!
//! // Create a `Transport`.
//! let transport = TcpConfig::new()
//!     .upgrade(upgrade::Version::V1)
//!     .authenticate(SecioConfig::new(local_keys.clone()))
//!     .multiplex(MplexConfig::default());
//!
//! // The transport can be used with a `Network` from `libp2p-core`, or a
//! // `Swarm` from from `libp2p-swarm`. See the documentation of these
//! // crates for mode details.
//!
//! // let network = Network::new(transport, local_keys.public().into_peer_id());
//! // let swarm = Swarm::new(transport, behaviour, local_keys.public().into_peer_id());
//! # }
//! ```
//!

pub use self::error::SecioError;

use futures::stream::MapErr as StreamMapErr;
use futures::{prelude::*, io::Initializer};
use libp2p_core::{PeerId, PublicKey, identity, upgrade::{UpgradeInfo, InboundUpgrade, OutboundUpgrade, Negotiated}};
use log::debug;
use rw_stream_sink::RwStreamSink;
use std::{io, iter, pin::Pin, task::Context, task::Poll};

mod algo_support;
mod codec;
mod error;
mod exchange;
mod handshake;
// #[allow(rust_2018_idioms)]
mod structs_proto;
mod stream_cipher;

pub use crate::algo_support::Digest;
pub use crate::exchange::KeyAgreement;
pub use crate::stream_cipher::Cipher;

/// Implementation of the `ConnectionUpgrade` trait of `libp2p_core`. Automatically applies
/// secio on any connection.
#[derive(Clone)]
pub struct SecioConfig {
    /// Private and public keys of the local node.
    pub(crate) key: identity::Keypair,
    pub(crate) agreements_prop: Option<String>,
    pub(crate) ciphers_prop: Option<String>,
    pub(crate) digests_prop: Option<String>
}

impl SecioConfig {
    /// Create a new `SecioConfig` with the given keypair.
    pub fn new(kp: identity::Keypair) -> Self {
        SecioConfig {
            key: kp,
            agreements_prop: None,
            ciphers_prop: None,
            digests_prop: None
        }
    }

    /// Override the default set of supported key agreement algorithms.
    pub fn key_agreements<'a, I>(mut self, xs: I) -> Self
    where
        I: IntoIterator<Item=&'a KeyAgreement>
    {
        self.agreements_prop = Some(algo_support::key_agreements_proposition(xs));
        self
    }

    /// Override the default set of supported ciphers.
    pub fn ciphers<'a, I>(mut self, xs: I) -> Self
    where
        I: IntoIterator<Item=&'a Cipher>
    {
        self.ciphers_prop = Some(algo_support::ciphers_proposition(xs));
        self
    }

    /// Override the default set of supported digest algorithms.
    pub fn digests<'a, I>(mut self, xs: I) -> Self
    where
        I: IntoIterator<Item=&'a Digest>
    {
        self.digests_prop = Some(algo_support::digests_proposition(xs));
        self
    }

    fn handshake<T>(self, socket: T) -> impl Future<Output = Result<(PeerId, SecioOutput<T>), SecioError>>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static
    {
        debug!("Starting secio upgrade");
        SecioMiddleware::handshake(socket, self)
            .map_ok(|(stream_sink, pubkey, ephemeral)| {
                let mapped = stream_sink.map_err(map_err as fn(_) -> _);
                let peer = pubkey.clone().into_peer_id();
                let io = SecioOutput {
                    stream: RwStreamSink::new(mapped),
                    remote_key: pubkey,
                    ephemeral_public_key: ephemeral
                };
                (peer, io)
            })
    }
}

/// Output of the secio protocol.
pub struct SecioOutput<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// The encrypted stream.
    pub stream: RwStreamSink<StreamMapErr<SecioMiddleware<S>, fn(SecioError) -> io::Error>>,
    /// The public key of the remote.
    pub remote_key: PublicKey,
    /// Ephemeral public key used during the negotiation.
    pub ephemeral_public_key: Vec<u8>,
}

impl UpgradeInfo for SecioConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/secio/1.0.0")
    }
}

impl<T> InboundUpgrade<T> for SecioConfig
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    type Output = (PeerId, SecioOutput<Negotiated<T>>);
    type Error = SecioError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        Box::pin(self.handshake(socket))
    }
}

impl<T> OutboundUpgrade<T> for SecioConfig
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    type Output = (PeerId, SecioOutput<Negotiated<T>>);
    type Error = SecioError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        Box::pin(self.handshake(socket))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for SecioOutput<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8])
        -> Poll<Result<usize, io::Error>>
    {
        AsyncRead::poll_read(Pin::new(&mut self.stream), cx, buf)
    }

    unsafe fn initializer(&self) -> Initializer {
        self.stream.initializer()
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for SecioOutput<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8])
        -> Poll<Result<usize, io::Error>>
    {
        AsyncWrite::poll_write(Pin::new(&mut self.stream), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context)
        -> Poll<Result<(), io::Error>>
    {
        AsyncWrite::poll_flush(Pin::new(&mut self.stream), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context)
        -> Poll<Result<(), io::Error>>
    {
        AsyncWrite::poll_close(Pin::new(&mut self.stream), cx)
    }
}

fn map_err(err: SecioError) -> io::Error {
    debug!("error during secio handshake {:?}", err);
    io::Error::new(io::ErrorKind::InvalidData, err)
}

/// Wraps around an object that implements `AsyncRead` and `AsyncWrite`.
///
/// Implements `Sink` and `Stream` whose items are frames of data. Each frame is encoded
/// individually, so you are encouraged to group data in few frames if possible.
pub struct SecioMiddleware<S> {
    inner: codec::FullCodec<S>,
}

impl<S> SecioMiddleware<S>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Attempts to perform a handshake on the given socket.
    ///
    /// On success, produces a `SecioMiddleware` that can then be used to encode/decode
    /// communications, plus the public key of the remote, plus the ephemeral public key.
    pub fn handshake(socket: S, config: SecioConfig)
        -> impl Future<Output = Result<(SecioMiddleware<S>, PublicKey, Vec<u8>), SecioError>>
    {
        handshake::handshake(socket, config).map_ok(|(inner, pubkey, ephemeral)| {
            let inner = SecioMiddleware { inner };
            (inner, pubkey, ephemeral)
        })
    }
}

impl<S> Sink<Vec<u8>> for SecioMiddleware<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.inner), cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.inner), item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.inner), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.inner), cx)
    }
}

impl<S> Stream for SecioMiddleware<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    type Item = Result<Vec<u8>, SecioError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.inner), cx)
    }
}
