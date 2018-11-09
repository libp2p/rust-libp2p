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
//! extern crate tokio;
//! extern crate tokio_io;
//! extern crate libp2p_core;
//! extern crate libp2p_secio;
//! extern crate libp2p_tcp_transport;
//!
//! # fn main() {
//! use futures::Future;
//! use libp2p_secio::{SecioConfig, SecioKeyPair, SecioOutput};
//! use libp2p_core::{Multiaddr, Transport, upgrade};
//! use libp2p_tcp_transport::TcpConfig;
//! use tokio_io::io::write_all;
//! use tokio::runtime::current_thread::Runtime;
//!
//! let transport = TcpConfig::new()
//!     .with_upgrade({
//!         # let private_key = b"";
//!         //let private_key = include_bytes!("test-rsa-private-key.pk8");
//!         # let public_key = vec![];
//!         //let public_key = include_bytes!("test-rsa-public-key.der").to_vec();
//!         // See the documentation of `SecioKeyPair`.
//!         let keypair = SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap();
//!         let upgrade = SecioConfig::new(keypair);
//!
//!         upgrade::map(upgrade, |out: SecioOutput<_>| out.stream)
//!     });
//!
//! let future = transport.dial("/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap())
//!        .unwrap_or_else(|_| panic!("Unable to dial node"))
//!     .and_then(|connection| {
//!         // Sends "hello world" on the connection, will be encrypted.
//!         write_all(connection, "hello world")
//!     });
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

extern crate aes_ctr;
#[cfg(feature = "secp256k1")]
extern crate asn1_der;
extern crate bytes;
extern crate ctr;
extern crate ed25519_dalek;
extern crate futures;
extern crate hmac;
extern crate libp2p_core;
#[macro_use]
extern crate log;
extern crate protobuf;
extern crate rand;
#[cfg(not(target_os = "emscripten"))]
extern crate ring;
extern crate rw_stream_sink;
#[cfg(feature = "secp256k1")]
extern crate secp256k1;
extern crate sha2;
#[cfg(target_os = "emscripten")]
#[macro_use]
extern crate stdweb;
extern crate tokio_io;
extern crate twofish;
#[cfg(not(target_os = "emscripten"))]
extern crate untrusted;

#[cfg(feature = "aes-all")]
#[macro_use]
extern crate lazy_static;
pub use self::error::SecioError;

#[cfg(feature = "secp256k1")]
use asn1_der::{traits::FromDerEncoded, traits::FromDerObject, DerObject};
use bytes::{Bytes, BytesMut};
use ed25519_dalek::Keypair as Ed25519KeyPair;
use futures::stream::MapErr as StreamMapErr;
use futures::{Future, Poll, Sink, StartSend, Stream};
use libp2p_core::{PeerId, PublicKey};
#[cfg(all(feature = "rsa", not(target_os = "emscripten")))]
use ring::signature::RSAKeyPair;
use rw_stream_sink::RwStreamSink;
use std::error::Error;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};
#[cfg(all(feature = "rsa", not(target_os = "emscripten")))]
use untrusted::Input;

mod algo_support;
mod codec;
mod error;
mod exchange;
mod handshake;
mod structs_proto;
mod stream_cipher;

pub use algo_support::Digest;
pub use exchange::KeyAgreement;
pub use stream_cipher::Cipher;

/// Implementation of the `ConnectionUpgrade` trait of `libp2p_core`. Automatically applies
/// secio on any connection.
#[derive(Clone)]
pub struct SecioConfig {
    /// Private and public keys of the local node.
    pub(crate) key: SecioKeyPair,
    pub(crate) agreements_prop: Option<String>,
    pub(crate) ciphers_prop: Option<String>,
    pub(crate) digests_prop: Option<String>
}

impl SecioConfig {
    /// Create a new `SecioConfig` with the given keypair.
    pub fn new(kp: SecioKeyPair) -> Self {
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
}

/// Private and public keys of the local node.
///
/// # Generating offline keys with OpenSSL
///
/// ## RSA
///
/// Generating the keys:
///
/// ```text
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
    /// Builds a `SecioKeyPair` from a PKCS8 private key and public key.
    #[cfg(all(feature = "ring", not(target_os = "emscripten")))]
    pub fn rsa_from_pkcs8<P>(
        private: &[u8],
        public: P,
    ) -> Result<SecioKeyPair, Box<Error + Send + Sync>>
    where
        P: Into<Vec<u8>>,
    {
        let private = RSAKeyPair::from_pkcs8(Input::from(&private[..])).map_err(Box::new)?;

        Ok(SecioKeyPair {
            inner: SecioKeyPairInner::Rsa {
                public: public.into(),
                private: Arc::new(private),
            },
        })
    }

    /// Generates a new Ed25519 key pair and uses it.
    pub fn ed25519_generated() -> Result<SecioKeyPair, Box<Error + Send + Sync>> {
        let mut csprng = rand::OsRng::new()?;
        let keypair: Ed25519KeyPair = Ed25519KeyPair::generate::<sha2::Sha512, _>(&mut csprng);
        Ok(SecioKeyPair {
            inner: SecioKeyPairInner::Ed25519 {
                key_pair: Arc::new(keypair),
            }
        })
    }

    /// Generates a new random sec256k1 key pair.
    #[cfg(feature = "secp256k1")]
    pub fn secp256k1_generated() -> Result<SecioKeyPair, Box<Error + Send + Sync>> {
        let secp = secp256k1::Secp256k1::new();
        // TODO: This will work once 0.11.5 is released. See https://github.com/rust-bitcoin/rust-secp256k1/pull/80#pullrequestreview-172681778
        // let private = secp256k1::key::SecretKey::new(&secp, &mut secp256k1::rand::thread_rng());
        use rand::Rng;
        let mut random_slice= [0u8; secp256k1::constants::SECRET_KEY_SIZE];
        rand::thread_rng().fill(&mut random_slice[..]);
        let private = secp256k1::key::SecretKey::from_slice(&secp, &random_slice).expect("slice has the right size");
        Ok(SecioKeyPair {
            inner: SecioKeyPairInner::Secp256k1 { private },
        })
    }

    /// Builds a `SecioKeyPair` from a raw secp256k1 32 bytes private key.
    #[cfg(feature = "secp256k1")]
    pub fn secp256k1_raw_key<K>(key: K) -> Result<SecioKeyPair, Box<Error + Send + Sync>>
    where
        K: AsRef<[u8]>,
    {
        let secp = secp256k1::Secp256k1::without_caps();
        let private = secp256k1::key::SecretKey::from_slice(&secp, key.as_ref())?;

        Ok(SecioKeyPair {
            inner: SecioKeyPairInner::Secp256k1 { private },
        })
    }

    /// Builds a `SecioKeyPair` from a secp256k1 private key in DER format.
    #[cfg(feature = "secp256k1")]
    pub fn secp256k1_from_der<K>(key: K) -> Result<SecioKeyPair, Box<Error + Send + Sync>>
    where
        K: AsRef<[u8]>,
    {
        // See ECPrivateKey in https://tools.ietf.org/html/rfc5915
        let obj: Vec<DerObject> =
            FromDerEncoded::with_der_encoded(key.as_ref()).map_err(|err| err.to_string())?;
        let priv_key_obj = obj.into_iter()
            .nth(1)
            .ok_or_else(|| "Not enough elements in DER".to_string())?;
        let private_key: Vec<u8> =
            FromDerObject::from_der_object(priv_key_obj).map_err(|err| err.to_string())?;
        SecioKeyPair::secp256k1_raw_key(&private_key)
    }

    /// Returns the public key corresponding to this key pair.
    pub fn to_public_key(&self) -> PublicKey {
        match self.inner {
            #[cfg(all(feature = "ring", not(target_os = "emscripten")))]
            SecioKeyPairInner::Rsa { ref public, .. } => PublicKey::Rsa(public.clone()),
            SecioKeyPairInner::Ed25519 { ref key_pair } => {
                PublicKey::Ed25519(key_pair.public.as_bytes().to_vec())
            }
            #[cfg(feature = "secp256k1")]
            SecioKeyPairInner::Secp256k1 { ref private } => {
                let secp = secp256k1::Secp256k1::signing_only();
                let pubkey = secp256k1::key::PublicKey::from_secret_key(&secp, private);
                PublicKey::Secp256k1(pubkey.serialize().to_vec())
            }
        }
    }

    /// Builds a `PeerId` corresponding to the public key of this key pair.
    #[inline]
    pub fn to_peer_id(&self) -> PeerId {
        self.to_public_key().into_peer_id()
    }

    // TODO: method to save generated key on disk?
}

// Inner content of `SecioKeyPair`.
#[derive(Clone)]
enum SecioKeyPairInner {
    #[cfg(all(feature = "ring", not(target_os = "emscripten")))]
    Rsa {
        public: Vec<u8>,
        // We use an `Arc` so that we can clone the enum.
        private: Arc<RSAKeyPair>,
    },
    Ed25519 {
        // We use an `Arc` so that we can clone the enum.
        key_pair: Arc<Ed25519KeyPair>,
    },
    #[cfg(feature = "secp256k1")]
    Secp256k1 { private: secp256k1::key::SecretKey },
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

impl<S> libp2p_core::ConnectionUpgrade<S> for SecioConfig
where
    S: AsyncRead + AsyncWrite + Send + 'static, // TODO: 'static :(
{
    type Output = SecioOutput<S>;
    type Future = Box<Future<Item = Self::Output, Error = IoError> + Send>;
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
        _: libp2p_core::Endpoint,
    ) -> Self::Future {
        debug!("Starting secio upgrade");

        let fut = SecioMiddleware::handshake(incoming, self);
        let wrapped = fut.map(|(stream_sink, pubkey, ephemeral)| {
            let mapped = stream_sink.map_err(map_err as fn(_) -> _);
            SecioOutput {
                stream: RwStreamSink::new(mapped),
                remote_key: pubkey,
                ephemeral_public_key: ephemeral,
            }
        }).map_err(map_err);
        Box::new(wrapped)
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
    pub fn handshake<'a>(
        socket: S,
        config: SecioConfig,
    ) -> Box<Future<Item = (SecioMiddleware<S>, PublicKey, Vec<u8>), Error = SecioError> + Send + 'a>
    where
        S: 'a,
    {
        let fut = handshake::handshake(socket, config).map(|(inner, pubkey, ephemeral)| {
            let inner = SecioMiddleware { inner };
            (inner, pubkey, ephemeral)
        });

        Box::new(fut)
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
