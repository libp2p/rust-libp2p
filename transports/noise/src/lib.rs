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
//! use libp2p::tcp::TcpTransport;
//! use libp2p::noise::{Keypair, X25519Spec, NoiseAuthenticated};
//!
//! # fn main() {
//! let id_keys = identity::Keypair::generate_ed25519();
//! let noise = NoiseAuthenticated::xx(&id_keys).unwrap();
//! let builder = TcpTransport::default().upgrade(upgrade::Version::V1).authenticate(noise);
//! // let transport = builder.multiplex(...);
//! # }
//! ```
//!
//! [noise]: http://noiseprotocol.org/

mod error;
mod io;
mod protocol;

pub use error::NoiseError;
pub use io::handshake;
pub use io::handshake::{Handshake, IdentityExchange, RemoteIdentity};
pub use io::NoiseOutput;
pub use protocol::{x25519::X25519, x25519_spec::X25519Spec};
pub use protocol::{AuthenticKeypair, Keypair, KeypairIdentity, PublicKey, SecretKey};
pub use protocol::{Protocol, ProtocolParams, IK, IX, XX};

use futures::prelude::*;
use libp2p_core::{identity, InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use snow::HandshakeState;
use std::pin::Pin;
use zeroize::Zeroize;

/// The protocol upgrade configuration.
#[derive(Clone)]
pub struct NoiseConfig<P, C: Zeroize, R = ()> {
    dh_keys: AuthenticKeypair<C>,
    params: ProtocolParams,
    legacy: LegacyConfig,
    remote: R,
    _marker: std::marker::PhantomData<P>,

    /// Prologue to use in the noise handshake.
    ///
    /// The prologue can contain arbitrary data that will be hashed into the noise handshake.
    /// For the handshake to succeed, both parties must set the same prologue.
    ///
    /// For further information, see <https://noiseprotocol.org/noise.html#prologue>.
    prologue: Vec<u8>,
}

impl<H, C: Zeroize, R> NoiseConfig<H, C, R> {
    /// Turn the `NoiseConfig` into an authenticated upgrade for use
    /// with a `Swarm`.
    pub fn into_authenticated(self) -> NoiseAuthenticated<H, C, R> {
        NoiseAuthenticated { config: self }
    }

    /// Set the noise prologue.
    pub fn with_prologue(self, prologue: Vec<u8>) -> Self {
        Self { prologue, ..self }
    }

    /// Sets the legacy configuration options to use, if any.
    pub fn set_legacy_config(&mut self, cfg: LegacyConfig) -> &mut Self {
        self.legacy = cfg;
        self
    }
}

impl<H, C, R> NoiseConfig<H, C, R>
where
    C: Zeroize + AsRef<[u8]>,
{
    fn into_responder(self) -> Result<HandshakeState, NoiseError> {
        let state = self
            .params
            .into_builder()
            .prologue(self.prologue.as_ref())
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from)?;

        Ok(state)
    }

    fn into_initiator(self) -> Result<HandshakeState, NoiseError> {
        let state = self
            .params
            .into_builder()
            .prologue(self.prologue.as_ref())
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_initiator()
            .map_err(NoiseError::from)?;

        Ok(state)
    }
}

impl<C> NoiseConfig<IX, C>
where
    C: Protocol<C> + Zeroize,
{
    /// Create a new `NoiseConfig` for the `IX` handshake pattern.
    pub fn ix(dh_keys: AuthenticKeypair<C>) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_ix(),
            legacy: LegacyConfig::default(),
            remote: (),
            _marker: std::marker::PhantomData,
            prologue: Vec::default(),
        }
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
            legacy: LegacyConfig::default(),
            remote: (),
            _marker: std::marker::PhantomData,
            prologue: Vec::default(),
        }
    }
}

impl<C> NoiseConfig<IK, C>
where
    C: Protocol<C> + Zeroize,
{
    /// Create a new `NoiseConfig` for the `IK` handshake pattern (recipient side).
    ///
    /// Since the identity of the local node is known to the remote, this configuration
    /// does not transmit a static DH public key or public identity key to the remote.
    pub fn ik_listener(dh_keys: AuthenticKeypair<C>) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_ik(),
            legacy: LegacyConfig::default(),
            remote: (),
            _marker: std::marker::PhantomData,
            prologue: Vec::default(),
        }
    }
}

impl<C> NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>
where
    C: Protocol<C> + Zeroize,
{
    /// Create a new `NoiseConfig` for the `IK` handshake pattern (initiator side).
    ///
    /// In this configuration, the remote identity is known to the local node,
    /// but the local node still needs to transmit its own public identity.
    pub fn ik_dialer(
        dh_keys: AuthenticKeypair<C>,
        remote_id: identity::PublicKey,
        remote_dh: PublicKey<C>,
    ) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_ik(),
            legacy: LegacyConfig::default(),
            remote: (remote_dh, remote_id),
            _marker: std::marker::PhantomData,
            prologue: Vec::default(),
        }
    }
}

// Handshake pattern IX /////////////////////////////////////////////////////

impl<T, C> InboundUpgrade<T> for NoiseConfig<IX, C>
where
    NoiseConfig<IX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        let config = self.legacy;
        let identity = self.dh_keys.clone().into_identity();

        handshake::rt1_responder(
            socket,
            self.into_responder(),
            identity,
            IdentityExchange::Mutual,
            config,
        )
    }
}

impl<T, C> OutboundUpgrade<T> for NoiseConfig<IX, C>
where
    NoiseConfig<IX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        let legacy = self.legacy;
        let identity = self.dh_keys.clone().into_identity();

        handshake::rt1_initiator(
            socket,
            self.into_initiator(),
            identity,
            IdentityExchange::Mutual,
            legacy,
        )
    }
}

// Handshake pattern XX /////////////////////////////////////////////////////

impl<T, C> InboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        let legacy = self.legacy;
        let identity = self.dh_keys.clone().into_identity();

        handshake::rt15_responder(
            socket,
            self.into_responder(),
            identity,
            IdentityExchange::Mutual,
            legacy,
        )
    }
}

impl<T, C> OutboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        let legacy = self.legacy;
        let identity = self.dh_keys.clone().into_identity();

        handshake::rt15_initiator(
            socket,
            self.into_initiator(),
            identity,
            IdentityExchange::Mutual,
            legacy,
        )
    }
}

// Handshake pattern IK /////////////////////////////////////////////////////

impl<T, C, R> InboundUpgrade<T> for NoiseConfig<IK, C, R>
where
    NoiseConfig<IK, C, R>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        let legacy = self.legacy;
        let identity = self.dh_keys.clone().into_identity();

        handshake::rt1_responder(
            socket,
            self.into_responder(),
            identity,
            IdentityExchange::Receive,
            legacy,
        )
    }
}

impl<T, C> OutboundUpgrade<T> for NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>
where
    NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Clone + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Handshake<T, C>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = self
            .params
            .into_builder()
            .prologue(self.prologue.as_ref())
            .local_private_key(self.dh_keys.secret().as_ref())
            .remote_public_key(self.remote.0.as_ref())
            .build_initiator()
            .map_err(NoiseError::from);

        handshake::rt1_initiator(
            socket,
            session,
            self.dh_keys.into_identity(),
            IdentityExchange::Send {
                remote: self.remote.1,
            },
            self.legacy,
        )
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
pub struct NoiseAuthenticated<P, C: Zeroize, R> {
    config: NoiseConfig<P, C, R>,
}

impl NoiseAuthenticated<XX, X25519, ()> {
    /// Create a new [`NoiseAuthenticated`] for the `XX` handshake pattern using X25519 DH keys.
    ///
    /// For now, this is the only combination that is guaranteed to be compatible with other libp2p implementations.
    pub fn xx(id_keys: &identity::Keypair) -> Result<Self, NoiseError> {
        let dh_keys = Keypair::<X25519>::new();
        let noise_keys = dh_keys.into_authentic(id_keys)?;
        let config = NoiseConfig::xx(noise_keys);

        Ok(config.into_authenticated())
    }
}

impl<P, C: Zeroize, R> UpgradeInfo for NoiseAuthenticated<P, C, R>
where
    NoiseConfig<P, C, R>: UpgradeInfo,
{
    type Info = <NoiseConfig<P, C, R> as UpgradeInfo>::Info;
    type InfoIter = <NoiseConfig<P, C, R> as UpgradeInfo>::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.config.protocol_info()
    }
}

impl<T, P, C, R> InboundUpgrade<T> for NoiseAuthenticated<P, C, R>
where
    NoiseConfig<P, C, R>: UpgradeInfo
        + InboundUpgrade<T, Output = (RemoteIdentity<C>, NoiseOutput<T>), Error = NoiseError>
        + 'static,
    <NoiseConfig<P, C, R> as InboundUpgrade<T>>::Future: Send,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (PeerId, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: T, info: Self::Info) -> Self::Future {
        Box::pin(
            self.config
                .upgrade_inbound(socket, info)
                .and_then(|(remote, io)| match remote {
                    RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
                    _ => future::err(NoiseError::AuthenticationFailed),
                }),
        )
    }
}

impl<T, P, C, R> OutboundUpgrade<T> for NoiseAuthenticated<P, C, R>
where
    NoiseConfig<P, C, R>: UpgradeInfo
        + OutboundUpgrade<T, Output = (RemoteIdentity<C>, NoiseOutput<T>), Error = NoiseError>
        + 'static,
    <NoiseConfig<P, C, R> as OutboundUpgrade<T>>::Future: Send,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (PeerId, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: T, info: Self::Info) -> Self::Future {
        Box::pin(
            self.config
                .upgrade_outbound(socket, info)
                .and_then(|(remote, io)| match remote {
                    RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
                    _ => future::err(NoiseError::AuthenticationFailed),
                }),
        )
    }
}

/// Legacy configuration options.
#[derive(Clone, Copy, Default)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_hashes_disagree_if_prologue_differs() {
        let alice = new_xx_config()
            .with_prologue(b"alice prologue".to_vec())
            .into_initiator()
            .unwrap();
        let bob = new_xx_config()
            .with_prologue(b"bob prologue".to_vec())
            .into_responder()
            .unwrap();

        let alice_handshake_hash = alice.get_handshake_hash();
        let bob_handshake_hash = bob.get_handshake_hash();

        assert_ne!(alice_handshake_hash, bob_handshake_hash)
    }

    #[test]
    fn handshake_hashes_agree_if_prologue_is_the_same() {
        let alice = new_xx_config()
            .with_prologue(b"shared knowledge".to_vec())
            .into_initiator()
            .unwrap();
        let bob = new_xx_config()
            .with_prologue(b"shared knowledge".to_vec())
            .into_responder()
            .unwrap();

        let alice_handshake_hash = alice.get_handshake_hash();
        let bob_handshake_hash = bob.get_handshake_hash();

        assert_eq!(alice_handshake_hash, bob_handshake_hash)
    }

    fn new_xx_config() -> NoiseConfig<XX, X25519> {
        let dh_keys = Keypair::<X25519>::new();
        let noise_keys = dh_keys
            .into_authentic(&identity::Keypair::generate_ed25519())
            .unwrap();

        NoiseConfig::xx(noise_keys)
    }
}
