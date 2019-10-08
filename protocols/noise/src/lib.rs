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
//! over a particular choice of DH key agreement (currently only X25519).
//!
//! All upgrades produce as output a pair, consisting of the remote's static public key
//! and a `NoiseOutput` which represents the established cryptographic session with the
//! remote, implementing `tokio_io::AsyncRead` and `tokio_io::AsyncWrite`.
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! use libp2p_core::{identity, Transport, upgrade};
//! use libp2p_tcp::TcpConfig;
//! use libp2p_noise::{Keypair, X25519, NoiseConfig};
//!
//! # fn main() {
//! let id_keys = identity::Keypair::generate_ed25519();
//! let dh_keys = Keypair::<X25519>::new().into_authentic(&id_keys).unwrap();
//! let noise = NoiseConfig::xx(dh_keys).into_authenticated();
//! let builder = TcpConfig::new().upgrade(upgrade::Version::V1).authenticate(noise);
//! // let transport = builder.multiplex(...);
//! # }
//! ```
//!
//! [noise]: http://noiseprotocol.org/

mod error;
mod io;
mod protocol;

pub use error::NoiseError;
pub use io::NoiseOutput;
pub use io::handshake::{Handshake, RemoteIdentity, IdentityExchange};
pub use protocol::{Keypair, AuthenticKeypair, KeypairIdentity, PublicKey, SecretKey};
pub use protocol::{Protocol, ProtocolParams, x25519::X25519, IX, IK, XX};

use futures::{future::{self, FutureResult}, Future};
use libp2p_core::{identity, PeerId, UpgradeInfo, InboundUpgrade, OutboundUpgrade, Negotiated};
use tokio_io::{AsyncRead, AsyncWrite};
use zeroize::Zeroize;

/// The protocol upgrade configuration.
#[derive(Clone)]
pub struct NoiseConfig<P, C: Zeroize, R = ()> {
    dh_keys: AuthenticKeypair<C>,
    params: ProtocolParams,
    remote: R,
    _marker: std::marker::PhantomData<P>
}

impl<H, C: Zeroize, R> NoiseConfig<H, C, R> {
    /// Turn the `NoiseConfig` into an authenticated upgrade for use
    /// with a [`Network`](libp2p_core::nodes::Network).
    pub fn into_authenticated(self) -> NoiseAuthenticated<H, C, R> {
        NoiseAuthenticated { config: self }
    }
}

impl<C> NoiseConfig<IX, C>
where
    C: Protocol<C> + Zeroize
{
    /// Create a new `NoiseConfig` for the `IX` handshake pattern.
    pub fn ix(dh_keys: AuthenticKeypair<C>) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_ix(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl<C> NoiseConfig<XX, C>
where
    C: Protocol<C> + Zeroize
{
    /// Create a new `NoiseConfig` for the `XX` handshake pattern.
    pub fn xx(dh_keys: AuthenticKeypair<C>) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_xx(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl<C> NoiseConfig<IK, C>
where
    C: Protocol<C> + Zeroize
{
    /// Create a new `NoiseConfig` for the `IK` handshake pattern (recipient side).
    ///
    /// Since the identity of the local node is known to the remote, this configuration
    /// does not transmit a static DH public key or public identity key to the remote.
    pub fn ik_listener(dh_keys: AuthenticKeypair<C>) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_ik(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl<C> NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>
where
    C: Protocol<C> + Zeroize
{
    /// Create a new `NoiseConfig` for the `IK` handshake pattern (initiator side).
    ///
    /// In this configuration, the remote identity is known to the local node,
    /// but the local node still needs to transmit its own public identity.
    pub fn ik_dialer(
        dh_keys: AuthenticKeypair<C>,
        remote_id: identity::PublicKey,
        remote_dh: PublicKey<C>
    ) -> Self {
        NoiseConfig {
            dh_keys,
            params: C::params_ik(),
            remote: (remote_dh, remote_id),
            _marker: std::marker::PhantomData
        }
    }
}

// Handshake pattern IX /////////////////////////////////////////////////////

impl<T, C> InboundUpgrade<T> for NoiseConfig<IX, C>
where
    NoiseConfig<IX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = Handshake<Negotiated<T>, C>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from);
        Handshake::rt1_responder(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Mutual)
    }
}

impl<T, C> OutboundUpgrade<T> for NoiseConfig<IX, C>
where
    NoiseConfig<IX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = Handshake<Negotiated<T>, C>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_initiator()
            .map_err(NoiseError::from);
        Handshake::rt1_initiator(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Mutual)
    }
}

// Handshake pattern XX /////////////////////////////////////////////////////

impl<T, C> InboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = Handshake<Negotiated<T>, C>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from);
        Handshake::rt15_responder(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Mutual)
    }
}

impl<T, C> OutboundUpgrade<T> for NoiseConfig<XX, C>
where
    NoiseConfig<XX, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = Handshake<Negotiated<T>, C>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_initiator()
            .map_err(NoiseError::from);
        Handshake::rt15_initiator(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Mutual)
    }
}

// Handshake pattern IK /////////////////////////////////////////////////////

impl<T, C> InboundUpgrade<T> for NoiseConfig<IK, C>
where
    NoiseConfig<IK, C>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = Handshake<Negotiated<T>, C>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from);
        Handshake::rt1_responder(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Receive)
    }
}

impl<T, C> OutboundUpgrade<T> for NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>
where
    NoiseConfig<IK, C, (PublicKey<C>, identity::PublicKey)>: UpgradeInfo,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = Handshake<Negotiated<T>, C>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        let session = self.params.into_builder()
            .local_private_key(self.dh_keys.secret().as_ref())
            .remote_public_key(self.remote.0.as_ref())
            .build_initiator()
            .map_err(NoiseError::from);
        Handshake::rt1_initiator(socket, session,
            self.dh_keys.into_identity(),
            IdentityExchange::Send { remote: self.remote.1 })
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
/// for creating an [`authenticated`](libp2p_core::TransportBuilder::authenticate)
/// transport for use with a [`Network`](libp2p_core::nodes::Network).
#[derive(Clone)]
pub struct NoiseAuthenticated<P, C: Zeroize, R> {
    config: NoiseConfig<P, C, R>
}

impl<P, C: Zeroize, R> UpgradeInfo for NoiseAuthenticated<P, C, R>
where
    NoiseConfig<P, C, R>: UpgradeInfo
{
    type Info = <NoiseConfig<P, C, R> as UpgradeInfo>::Info;
    type InfoIter = <NoiseConfig<P, C, R> as UpgradeInfo>::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.config.protocol_info()
    }
}

impl<T, P, C, R> InboundUpgrade<T> for NoiseAuthenticated<P, C, R>
where
    NoiseConfig<P, C, R>: UpgradeInfo + InboundUpgrade<T,
        Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>),
        Error = NoiseError
    >,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (PeerId, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = future::AndThen<
        <NoiseConfig<P, C, R> as InboundUpgrade<T>>::Future,
        FutureResult<Self::Output, Self::Error>,
        fn((RemoteIdentity<C>, NoiseOutput<Negotiated<T>>)) -> FutureResult<Self::Output, Self::Error>
    >;

    fn upgrade_inbound(self, socket: Negotiated<T>, info: Self::Info) -> Self::Future {
        self.config.upgrade_inbound(socket, info)
            .and_then(|(remote, io)| future::result(match remote {
                RemoteIdentity::IdentityKey(pk) => Ok((pk.into_peer_id(), io)),
                _ => Err(NoiseError::AuthenticationFailed)
            }))
    }
}

impl<T, P, C, R> OutboundUpgrade<T> for NoiseAuthenticated<P, C, R>
where
    NoiseConfig<P, C, R>: UpgradeInfo + OutboundUpgrade<T,
        Output = (RemoteIdentity<C>, NoiseOutput<Negotiated<T>>),
        Error = NoiseError
    >,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Zeroize + Send + 'static,
{
    type Output = (PeerId, NoiseOutput<Negotiated<T>>);
    type Error = NoiseError;
    type Future = future::AndThen<
        <NoiseConfig<P, C, R> as OutboundUpgrade<T>>::Future,
        FutureResult<Self::Output, Self::Error>,
        fn((RemoteIdentity<C>, NoiseOutput<Negotiated<T>>)) -> FutureResult<Self::Output, Self::Error>
    >;

    fn upgrade_outbound(self, socket: Negotiated<T>, info: Self::Info) -> Self::Future {
        self.config.upgrade_outbound(socket, info)
            .and_then(|(remote, io)| future::result(match remote {
                RemoteIdentity::IdentityKey(pk) => Ok((pk.into_peer_id(), io)),
                _ => Err(NoiseError::AuthenticationFailed)
            }))
    }
}

