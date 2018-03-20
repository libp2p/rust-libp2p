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
//! extern crate futures;
//! extern crate tokio_core;
//! extern crate tokio_io;
//! extern crate libp2p_swarm;
//! extern crate libp2p_secio;
//! extern crate libp2p_tcp_transport;
//!
//! # fn main() {
//! use futures::Future;
//! use libp2p_secio::{SecioConfig, SecioKeyPair};
//! use libp2p_swarm::{Multiaddr, Transport};
//! use libp2p_tcp_transport::TcpConfig;
//! use tokio_core::reactor::Core;
//! use tokio_io::io::write_all;
//!
//! let mut core = Core::new().unwrap();
//!
//! let transport = TcpConfig::new(core.handle())
//!     .with_upgrade({
//!         # let private_key = b"";
//!         //let private_key = include_bytes!("test-private-key.pk8");
//!         # let public_key = vec![];
//!         //let public_key = include_bytes!("test-public-key.der").to_vec();
//!         SecioConfig {
//!                // See the documentation of `SecioKeyPair`.
//!             key: SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
//!         }
//!     });
//!
//! let future = transport.dial("/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap())
//!        .unwrap_or_else(|_| panic!("Unable to dial node"))
//!     .and_then(|(connection, _)| {
//!         // Sends "hello world" on the connection, will be encrypted.
//!         write_all(connection, "hello world")
//!     });
//!
//! core.run(future).unwrap();
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

extern crate bytes;
extern crate crypto;
extern crate futures;
extern crate libp2p_swarm;
#[macro_use]
extern crate log;
extern crate protobuf;
extern crate rand;
extern crate ring;
extern crate rw_stream_sink;
extern crate tokio_io;
extern crate untrusted;

pub use self::error::SecioError;

use bytes::{Bytes, BytesMut};
use futures::{Future, Poll, Sink, StartSend, Stream};
use futures::stream::MapErr as StreamMapErr;
use libp2p_swarm::Multiaddr;
use ring::signature::RSAKeyPair;
use rw_stream_sink::RwStreamSink;
use std::error::Error;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};
use untrusted::Input;

mod algo_support;
mod codec;
mod error;
mod keys_proto;
mod handshake;
mod structs_proto;

/// Implementation of the `ConnectionUpgrade` trait of `libp2p_swarm`. Automatically applies
/// secio on any connection.
#[derive(Clone)]
pub struct SecioConfig {
    /// Private and public keys of the local node.
    pub key: SecioKeyPair,
}

/// Private and public keys of the local node.
///
/// # Generating offline keys with OpenSSL
///
/// ## RSA
///
/// Generating the keys:
///
/// ```ignore
/// openssl genrsa -out private.pem 2048
/// openssl rsa -in private.pem -outform DER -pubout -out public.der
/// openssl pkcs8 -in private.pem -topk8 -nocrypt -out private.pk8
/// rm private.pem      # optional
/// ```
///
/// Loading the keys:
///
/// ```ignore
/// let key_pair = SecioKeyPair::rsa_from_pkcs8(include_bytes!("private.pk8"),
///                                                include_bytes!("public.der"));
/// ```
///
#[derive(Clone)]
pub struct SecioKeyPair {
    inner: SecioKeyPairInner,
}

impl SecioKeyPair {
    pub fn rsa_from_pkcs8<P>(
        private: &[u8],
        public: P,
    ) -> Result<SecioKeyPair, Box<Error + Send + Sync>>
    where
        P: Into<Vec<u8>>,
    {
        let private =
            RSAKeyPair::from_pkcs8(Input::from(&private[..])).map_err(|err| Box::new(err))?;

        Ok(SecioKeyPair {
            inner: SecioKeyPairInner::Rsa {
                public: public.into(),
                private: Arc::new(private),
            },
        })
    }
}

// Inner content of `SecioKeyPair`.
#[derive(Clone)]
enum SecioKeyPairInner {
    Rsa {
        public: Vec<u8>,
        private: Arc<RSAKeyPair>,
    },
}

#[derive(Debug, Clone)]
pub enum SecioPublicKey<'a> {
    /// DER format.
    Rsa(&'a [u8]),
}

impl<S> libp2p_swarm::ConnectionUpgrade<S> for SecioConfig
where
    S: AsyncRead + AsyncWrite + 'static,
{
    type Output = RwStreamSink<StreamMapErr<SecioMiddleware<S>, fn(SecioError) -> IoError>>;
    type Future = Box<Future<Item = Self::Output, Error = IoError>>;
    type NamesIter = iter::Once<(Bytes, ())>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once(("/secio/1.0.0".into(), ()))
    }

    #[inline]
    fn upgrade(
        self,
        incoming: S,
        _: (),
        _: libp2p_swarm::Endpoint,
        remote_addr: &Multiaddr,
    ) -> Self::Future {
        info!(target: "libp2p-secio", "starting secio upgrade with {:?}", remote_addr);

        let fut = SecioMiddleware::handshake(incoming, self.key);
        let wrapped = fut.map(|stream_sink| {
            let mapped = stream_sink.map_err(map_err as fn(_) -> _);
            RwStreamSink::new(mapped)
        }).map_err(map_err);
        Box::new(wrapped)
    }
}

#[inline]
fn map_err(err: SecioError) -> IoError {
    debug!(target: "libp2p-secio", "error during secio handshake {:?}", err);
    IoError::new(IoErrorKind::InvalidData, err)
}

/// Wraps around an object that implements `AsyncRead` and `AsyncWrite`.
///
/// Implements `Sink` and `Stream` whose items are frames of data. Each frame is encoded
/// individually, so you are encouraged to group data in few frames if possible.
pub struct SecioMiddleware<S> {
    inner: codec::FullCodec<S>,
    remote_pubkey_der: Vec<u8>,
}

impl<S> SecioMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// Attempts to perform a handshake on the given socket.
    ///
    /// On success, produces a `SecioMiddleware` that can then be used to encode/decode
    /// communications.
    pub fn handshake<'a>(
        socket: S,
        key_pair: SecioKeyPair,
    ) -> Box<Future<Item = SecioMiddleware<S>, Error = SecioError> + 'a>
    where
        S: 'a,
    {
        let SecioKeyPairInner::Rsa { private, public } = key_pair.inner;

        let fut =
            handshake::handshake(socket, public, private).map(|(inner, pubkey)| SecioMiddleware {
                inner: inner,
                remote_pubkey_der: pubkey,
            });
        Box::new(fut)
    }

    /// Returns the public key of the remote in the `DER` format.
    #[inline]
    pub fn remote_public_key_der(&self) -> SecioPublicKey {
        SecioPublicKey::Rsa(&self.remote_pubkey_der)
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
