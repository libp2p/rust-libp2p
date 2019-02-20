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
//! # Connection upgrade
//!
//! The `SecioConfig` struct implements the `ConnectionUpgrade` trait. You can apply it over a
//! `Transport` by using the `with_upgrade` method. The returned object will also implement
//! `Transport` and will automatically apply the secio protocol over any connection that is opened
//! through it.
//!
//! ```no_run
//! # fn main() {
//! use futures::Future;
//! use libp2p_secio::{SecioConfig, SecioOutput};
//! use libp2p_core::{Multiaddr, identity, upgrade::apply_inbound};
//! use libp2p_core::transport::Transport;
//! use libp2p_tcp::TcpConfig;
//! use tokio_io::io::write_all;
//! use tokio::runtime::current_thread::Runtime;
//!
//! let dialer = TcpConfig::new()
//!     .with_upgrade({
//!         # let private_key = &mut [];
//!         // See the documentation of `identity::Keypair`.
//!         let keypair = identity::Keypair::rsa_from_pkcs8(private_key).unwrap();
//!         SecioConfig::new(keypair)
//!     })
//!     .map(|out: SecioOutput<_>, _| out.stream);
//!
//! let future = dialer.dial("/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap())
//!     .unwrap()
//!     .map_err(|e| panic!("error: {:?}", e))
//!     .and_then(|connection| {
//!         // Sends "hello world" on the connection, will be encrypted.
//!         write_all(connection, "hello world")
//!     })
//!     .map_err(|e| panic!("error: {:?}", e));
//!
//! let mut rt = Runtime::new().unwrap();
//! let _ = rt.block_on(future).unwrap();
//! # }
//! ```
//!
//! # Manual usage
//!
//! > **Note**: You are encouraged to use `SecioConfig` as described above.
//!
//! You can add the `secio` layer over a socket by calling `SecioMiddleware::handshake()`. This
//! method will perform a handshake with the host, and return a future that corresponds to the
//! moment when the handshake succeeds or errored. On success, the future produces a
//! `SecioMiddleware` that implements `Sink` and `Stream` and can be used to send packets of data.
//!

#![recursion_limit = "128"]

// TODO: unfortunately the `js!` macro of stdweb depends on tons of "private" macros, which we
//       don't want to import manually
#[cfg(any(target_os = "emscripten", target_os = "unknown"))]
#[macro_use]
extern crate stdweb;

pub use self::error::SecioError;

use bytes::BytesMut;
use futures::stream::MapErr as StreamMapErr;
use futures::{Future, Poll, Sink, StartSend, Stream};
use libp2p_core::{PublicKey, identity, upgrade::{UpgradeInfo, InboundUpgrade, OutboundUpgrade}};
use log::debug;
use rw_stream_sink::RwStreamSink;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};

mod algo_support;
mod codec;
mod error;
mod exchange;
mod handshake;
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

    fn handshake<T>(self, socket: T) -> impl Future<Item=SecioOutput<T>, Error=SecioError>
    where
        T: AsyncRead + AsyncWrite + Send + 'static
    {
        debug!("Starting secio upgrade");
        SecioMiddleware::handshake(socket, self)
            .map(|(stream_sink, pubkey, ephemeral)| {
                let mapped = stream_sink.map_err(map_err as fn(_) -> _);
                SecioOutput {
                    stream: RwStreamSink::new(mapped),
                    remote_key: pubkey,
                    ephemeral_public_key: ephemeral
                }
            })
    }
}

/// Output of the secio protocol.
pub struct SecioOutput<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// The encrypted stream.
    pub stream: RwStreamSink<StreamMapErr<SecioMiddleware<S>, fn(SecioError) -> IoError>>,
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
    T: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = SecioOutput<T>;
    type Error = SecioError;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        Box::new(self.handshake(socket))
    }
}

impl<T> OutboundUpgrade<T> for SecioConfig
where
    T: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = SecioOutput<T>;
    type Error = SecioError;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        Box::new(self.handshake(socket))
    }
}

#[inline]
fn map_err(err: SecioError) -> IoError {
    debug!("error during secio handshake {:?}", err);
    IoError::new(IoErrorKind::InvalidData, err)
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
    S: AsyncRead + AsyncWrite + Send,
{
    /// Attempts to perform a handshake on the given socket.
    ///
    /// On success, produces a `SecioMiddleware` that can then be used to encode/decode
    /// communications, plus the public key of the remote, plus the ephemeral public key.
    pub fn handshake(socket: S, config: SecioConfig)
        -> impl Future<Item = (SecioMiddleware<S>, PublicKey, Vec<u8>), Error = SecioError>
    {
        handshake::handshake(socket, config).map(|(inner, pubkey, ephemeral)| {
            let inner = SecioMiddleware { inner };
            (inner, pubkey, ephemeral)
        })
    }
}

impl<S> Sink for SecioMiddleware<S>
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

impl<S> Stream for SecioMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    type Item = Vec<u8>;
    type Error = SecioError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}
