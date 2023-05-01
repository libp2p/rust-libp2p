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

//! [Noise protocol framework][noise] support for libp2p.
//!
//! > **Note**: This crate is still experimental and subject to major breaking changes
//! >           both on the API and the wire protocol.
//!
//! This crate provides `libp2p_core::InboundUpgrade` and `libp2p_core::OutboundUpgrade`
//! implementations for various noise handshake patterns (currently `IK`, `IX`, and `XX`)
//! over a particular choice of Diffie–Hellman key agreement (currently only X25519).
//!
//! > **Note**: Only the `XX` handshake pattern is currently guaranteed to provide
//! >           interoperability with other libp2p implementations.
//!
//! All upgrades produce as output a pair, consisting of the remote's static public key
//! and a `NoiseOutput` which represents the established cryptographic session with the
//! remote, implementing `futures::io::AsyncRead` and `futures::io::AsyncWrite`.
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! use libp2p_core::{identity, Transport, upgrade};
//! use libp2p_tcp::TcpTransport;
//! use libp2p_noise as noise;
//!
//! # fn main() {
//! let id_keys = identity::Keypair::generate_ed25519();
//! let noise = noise::Config::new(&id_keys).unwrap();
//! let builder = TcpTransport::default().upgrade(upgrade::Version::V1).authenticate(noise);
//! // let transport = builder.multiplex(...);
//! # }
//! ```
//!
//! [noise]: http://noiseprotocol.org/

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![allow(deprecated)] // Temporarily until we remove deprecated items.

mod io;
mod protocol;

pub use io::handshake::RemoteIdentity;
pub use io::Output;
pub use protocol::x25519_spec::X25519Spec;
pub use protocol::{AuthenticKeypair, Keypair, KeypairIdentity, PublicKey, SecretKey};
pub use protocol::{Protocol, ProtocolParams, XX};

use crate::handshake::State;
use crate::io::handshake;
use futures::future::BoxFuture;
use futures::prelude::*;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use std::fmt;
use std::fmt::Formatter;
use std::pin::Pin;
use zeroize::Zeroize;

/// The configuration for the noise handshake.
#[derive(Clone)]
pub struct Config {
    inner: NoiseAuthenticated<XX, X25519Spec>,
}

impl Config {
    /// Construct a new configuration for the noise handshake using the XX handshake pattern.

    pub fn new(identity: &identity::Keypair) -> Result<Self, Error> {
        Ok(Config {
            inner: NoiseAuthenticated::xx(identity)?,
        })
    }

    /// Set the noise prologue.

    pub fn with_prologue(mut self, prologue: Vec<u8>) -> Self {
        self.inner.config.prologue = prologue;

        self
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise")
    }
}

impl<T> InboundUpgrade<T> for Config
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: T, info: Self::Info) -> Self::Future {
        self.inner.upgrade_inbound(socket, info)
    }
}

impl<T> OutboundUpgrade<T> for Config
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: T, info: Self::Info) -> Self::Future {
        self.inner.upgrade_outbound(socket, info)
    }
}

/// The protocol upgrade configuration.
#[deprecated(
    note = "Use `libp2p_noise::Config` instead. All other handshake patterns are deprecated and will be removed."
)]
#[derive(Clone)]
pub struct NoiseConfig<P, C: Zeroize> {
    dh_keys: AuthenticKeypair<C>,
    params: ProtocolParams,
    _marker: std::marker::PhantomData<P>,

    /// Prologue to use in the noise handshake.
    ///
    /// The prologue can contain arbitrary data that will be hashed into the noise handshake.
    /// For the handshake to succeed, both parties must set the same prologue.
    ///
    /// For further information, see <https://noiseprotocol.org/noise.html#prologue>.
    prologue: Vec<u8>,
}

impl<H, C: Zeroize> NoiseConfig<H, C> {
    /// Turn the `NoiseConfig` into an authenticated upgrade for use
    /// with a `Swarm`.
    pub fn into_authenticated(self) -> NoiseAuthenticated<H, C> {
        NoiseAuthenticated { config: self }
    }

    /// Set the noise prologue.
    pub fn with_prologue(self, prologue: Vec<u8>) -> Self {
        Self { prologue, ..self }
    }
}

/// Implement `into_responder` and `into_initiator` for all configs where `R = ()`.
///
/// This allows us to ignore the `remote` field.

impl<H, C> NoiseConfig<H, C>
where
    C: Zeroize + Protocol<C> + AsRef<[u8]>,
{
    fn into_responder<S>(self, socket: S) -> Result<State<S>, Error> {
        let session = self
            .params
            .into_builder(&self.prologue, self.dh_keys.keypair.secret(), None)
            .build_responder()?;

        let state = State::new(socket, session, self.dh_keys.identity, None);

        Ok(state)
    }

    fn into_initiator<S>(self, socket: S) -> Result<State<S>, Error> {
        let session = self
            .params
            .into_builder(&self.prologue, self.dh_keys.keypair.secret(), None)
            .build_initiator()?;

        let state = State::new(socket, session, self.dh_keys.identity, None);

        Ok(state)
    }
}

impl<C> NoiseConfig<XX, C>
where
    C: Protocol<C> + Zeroize,
{
    /// Create a new `NoiseConfig` for the `XX` handshake pattern.
    pub fn xx(dh_keys: AuthenticKeypair<C>) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_xx(),
            _marker: std::marker::PhantomData,
            prologue: Vec::default(),
        }
    }
}

/// libp2p_noise error type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Noise(#[from] snow::Error),
    #[error("Invalid public key")]
    InvalidKey(#[from] libp2p_identity::DecodingError),
    #[error("Only keys of length 32 bytes are supported")]
    InvalidLength,
    #[error("Remote authenticated with an unexpected public key")]
    UnexpectedKey,
    #[error("The signature of the remote identity's public key does not verify")]
    BadSignature,
    #[error("Authentication failed")]
    AuthenticationFailed,
    #[error(transparent)]
    InvalidPayload(DecodeError),
    #[error(transparent)]
    SigningError(#[from] libp2p_identity::SigningError),
}

#[derive(Debug, thiserror::Error)]
pub struct DecodeError(String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<quick_protobuf::Error> for DecodeError {
    fn from(e: quick_protobuf::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<quick_protobuf::Error> for Error {
    fn from(e: quick_protobuf::Error) -> Self {
        Error::InvalidPayload(e.into())
    }
}

/// Implements the responder part of the `XX` noise handshake pattern.
///
/// `XX` is a 1.5 round-trip (3 messages) handshake.
/// The first message in a noise handshake is unencrypted. In the `XX` handshake pattern, that message
/// is empty and thus does not leak any information. The identities are then exchanged in the second
/// and third message.
///
/// ```raw
/// initiator --{}--> responder
/// initiator <-{id}- responder
/// initiator -{id}-> responder
/// ```

impl<T, C> InboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, Output<T>);
    type Error = Error;
    type Future = BoxFuture<'static, Result<(RemoteIdentity<C>, Output<T>), Error>>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        async move {
            let mut state = self.into_responder(socket)?;

            handshake::recv_empty(&mut state).await?;
            handshake::send_identity(&mut state).await?;
            handshake::recv_identity(&mut state).await?;

            state.finish()
        }
        .boxed()
    }
}

/// Implements the initiator part of the `XX` noise handshake pattern.
///
/// `XX` is a 1.5 round-trip (3 messages) handshake.
/// The first message in a noise handshake is unencrypted. In the `XX` handshake pattern, that message
/// is empty and thus does not leak any information. The identities are then exchanged in the second
/// and third message.
///
/// ```raw
/// initiator --{}--> responder
/// initiator <-{id}- responder
/// initiator -{id}-> responder
/// ```

impl<T, C> OutboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, Output<T>);
    type Error = Error;
    type Future = BoxFuture<'static, Result<(RemoteIdentity<C>, Output<T>), Error>>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        async move {
            let mut state = self.into_initiator(socket)?;

            handshake::send_empty(&mut state).await?;
            handshake::recv_identity(&mut state).await?;
            handshake::send_identity(&mut state).await?;

            state.finish()
        }
        .boxed()
    }
}

// Authenticated Upgrades /////////////////////////////////////////////////////

/// A `NoiseAuthenticated` transport upgrade that wraps around any
/// `NoiseConfig` handshake and verifies that the remote identified with a
/// [`RemoteIdentity::IdentityKey`], aborting otherwise.
///
/// See [`NoiseConfig::into_authenticated`].
///
/// On success, the upgrade yields the [`PeerId`] obtained from the
/// `RemoteIdentity`. The output of this upgrade is thus directly suitable
/// for creating an [`authenticated`](libp2p_core::transport::upgrade::Authenticate)
/// transport for use with a `Swarm`.
#[derive(Clone)]
#[deprecated(
    note = "Use `libp2p_noise::Config` instead. All other handshake patterns are deprecated and will be removed."
)]
pub struct NoiseAuthenticated<P, C: Zeroize> {
    config: NoiseConfig<P, C>,
}

impl NoiseAuthenticated<XX, X25519Spec> {
    /// Create a new [`NoiseAuthenticated`] for the `XX` handshake pattern using X25519 DH keys.
    ///
    /// For now, this is the only combination that is guaranteed to be compatible with other libp2p implementations.
    #[deprecated(note = "Use `libp2p_noise::Config::new` instead.")]
    pub fn xx(id_keys: &identity::Keypair) -> Result<Self, Error> {
        let dh_keys = Keypair::<X25519Spec>::new();
        let noise_keys = dh_keys.into_authentic(id_keys)?;
        let config = NoiseConfig::xx(noise_keys);

        Ok(config.into_authenticated())
    }
}

impl<P, C: Zeroize> UpgradeInfo for NoiseAuthenticated<P, C>
where
    NoiseConfig<P, C>: UpgradeInfo,
{
    type Info = <NoiseConfig<P, C> as UpgradeInfo>::Info;
    type InfoIter = <NoiseConfig<P, C> as UpgradeInfo>::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.config.protocol_info()
    }
}

impl<T, P, C> InboundUpgrade<T> for NoiseAuthenticated<P, C>
where
    NoiseConfig<P, C>: UpgradeInfo
        + InboundUpgrade<T, Output = (RemoteIdentity<C>, Output<T>), Error = Error>
        + 'static,
    <NoiseConfig<P, C> as InboundUpgrade<T>>::Future: Send,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: T, info: Self::Info) -> Self::Future {
        Box::pin(
            self.config
                .upgrade_inbound(socket, info)
                .and_then(|(remote, io)| match remote {
                    RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
                    _ => future::err(Error::AuthenticationFailed),
                }),
        )
    }
}

impl<T, P, C> OutboundUpgrade<T> for NoiseAuthenticated<P, C>
where
    NoiseConfig<P, C>: UpgradeInfo
        + OutboundUpgrade<T, Output = (RemoteIdentity<C>, Output<T>), Error = Error>
        + 'static,
    <NoiseConfig<P, C> as OutboundUpgrade<T>>::Future: Send,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: T, info: Self::Info) -> Self::Future {
        Box::pin(
            self.config
                .upgrade_outbound(socket, info)
                .and_then(|(remote, io)| match remote {
                    RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
                    _ => future::err(Error::AuthenticationFailed),
                }),
        )
    }
}

/// Legacy configuration options.
#[derive(Clone, Copy, Default)]
#[deprecated(
    since = "0.42.0",
    note = "`LegacyConfig` will be removed without replacement."
)]
pub struct LegacyConfig {
    /// Whether to continue sending legacy handshake payloads,
    /// i.e. length-prefixed protobuf payloads inside a length-prefixed
    /// noise frame. These payloads are not interoperable with other
    /// libp2p implementations.
    pub send_legacy_handshake: bool,
    /// Whether to support receiving legacy handshake payloads,
    /// i.e. length-prefixed protobuf payloads inside a length-prefixed
    /// noise frame. These payloads are not interoperable with other
    /// libp2p implementations.
    pub recv_legacy_handshake: bool,
}
