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
//! This crate provides `libp2p_core::InboundUpgrade` and `libp2p_core::OutboundUpgrade`
//! implementations for various noise handshake patterns, currently IK, IX, and XX.
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
//! use libp2p_core::Transport;
//! use libp2p_tcp::TcpConfig;
//! use libp2p_noise::{Keypair, NoiseConfig};
//!
//! # fn main() {
//! let keypair = Keypair::gen_curve25519();
//! let transport = TcpConfig::new().with_upgrade(NoiseConfig::xx(keypair));
//! // ...
//! # }
//! ```
//!
//! [noise]: http://noiseprotocol.org/

mod error;
mod io;
mod keys;
mod util;

pub mod rt1;
pub mod rt15;

pub use error::NoiseError;
pub use io::NoiseOutput;
pub use keys::{Curve25519, PublicKey, SecretKey, Keypair};

use libp2p_core::{UpgradeInfo, InboundUpgrade, OutboundUpgrade};
use lazy_static::lazy_static;
use snow;
use tokio_io::{AsyncRead, AsyncWrite};
use util::Resolver;

lazy_static! {
    static ref PARAMS_IK: snow::params::NoiseParams = "Noise_IK_25519_ChaChaPoly_SHA256"
        .parse()
        .expect("valid pattern");

    static ref PARAMS_IX: snow::params::NoiseParams = "Noise_IX_25519_ChaChaPoly_SHA256"
        .parse()
        .expect("valid pattern");

    static ref PARAMS_XX: snow::params::NoiseParams = "Noise_XX_25519_ChaChaPoly_SHA256"
        .parse()
        .expect("valid pattern");
}

#[derive(Debug, Clone)]
pub enum IK {}

#[derive(Debug, Clone)]
pub enum IX {}

#[derive(Debug, Clone)]
pub enum XX {}

/// The protocol upgrade configuration.
#[derive(Clone)]
pub struct NoiseConfig<P, R = ()> {
    keypair: Keypair<Curve25519>,
    params: snow::params::NoiseParams,
    remote: R,
    _marker: std::marker::PhantomData<P>
}

impl NoiseConfig<IX> {
    /// Create a new `NoiseConfig` for the IX handshake pattern.
    pub fn ix(kp: Keypair<Curve25519>) -> Self {
        NoiseConfig {
            keypair: kp,
            params: PARAMS_IX.clone(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl NoiseConfig<XX> {
    /// Create a new `NoiseConfig` for the XX handshake pattern.
    pub fn xx(kp: Keypair<Curve25519>) -> Self {
        NoiseConfig {
            keypair: kp,
            params: PARAMS_XX.clone(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl NoiseConfig<IK> {
    /// Create a new `NoiseConfig` for the IK handshake pattern (recipient side).
    pub fn ik_listener(kp: Keypair<Curve25519>) -> Self {
        NoiseConfig {
            keypair: kp,
            params: PARAMS_IK.clone(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl NoiseConfig<IK, PublicKey<Curve25519>> {
    /// Create a new `NoiseConfig` for the IK handshake pattern (initiator side).
    pub fn ik_dialer(kp: Keypair<Curve25519>, remote: PublicKey<Curve25519>) -> Self {
        NoiseConfig {
            keypair: kp,
            params: PARAMS_IK.clone(),
            remote,
            _marker: std::marker::PhantomData
        }
    }
}

// Handshake pattern IX /////////////////////////////////////////////////////

impl UpgradeInfo for NoiseConfig<IX> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ix/25519/chachapoly/sha256/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for NoiseConfig<IX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = rt1::NoiseInboundFuture<T>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = snow::Builder::with_resolver(self.params, Box::new(Resolver))
            .local_private_key(self.keypair.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from);
        rt1::NoiseInboundFuture::new(socket, session)
    }
}

impl<T> OutboundUpgrade<T> for NoiseConfig<IX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = rt1::NoiseOutboundFuture<T>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = snow::Builder::with_resolver(self.params, Box::new(Resolver))
            .local_private_key(self.keypair.secret().as_ref())
            .build_initiator()
            .map_err(NoiseError::from);
        rt1::NoiseOutboundFuture::new(socket, session)
    }
}

// Handshake pattern XX /////////////////////////////////////////////////////

impl UpgradeInfo for NoiseConfig<XX> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/xx/25519/chachapoly/sha256/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for NoiseConfig<XX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = rt15::NoiseInboundFuture<T>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = snow::Builder::with_resolver(self.params, Box::new(Resolver))
            .local_private_key(self.keypair.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from);
        rt15::NoiseInboundFuture::new(socket, session)
    }
}

impl<T> OutboundUpgrade<T> for NoiseConfig<XX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = rt15::NoiseOutboundFuture<T>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = snow::Builder::with_resolver(self.params, Box::new(Resolver))
            .local_private_key(self.keypair.secret().as_ref())
            .build_initiator()
            .map_err(NoiseError::from);
        rt15::NoiseOutboundFuture::new(socket, session)
    }
}

// Handshake pattern IK /////////////////////////////////////////////////////

impl UpgradeInfo for NoiseConfig<IK> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ik/25519/chachapoly/sha256/0.1.0")
    }
}

impl UpgradeInfo for NoiseConfig<IK, PublicKey<Curve25519>> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ik/25519/chachapoly/sha256/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for NoiseConfig<IK>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = rt1::NoiseInboundFuture<T>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = snow::Builder::with_resolver(self.params, Box::new(Resolver))
            .local_private_key(self.keypair.secret().as_ref())
            .build_responder()
            .map_err(NoiseError::from);
        rt1::NoiseInboundFuture::new(socket, session)
    }
}

impl<T> OutboundUpgrade<T> for NoiseConfig<IK, PublicKey<Curve25519>>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = rt1::NoiseOutboundFuture<T>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        let session = snow::Builder::with_resolver(self.params, Box::new(Resolver))
            .local_private_key(self.keypair.secret().as_ref())
            .remote_public_key(self.remote.as_ref())
            .build_initiator()
            .map_err(NoiseError::from);
        rt1::NoiseOutboundFuture::new(socket, session)
    }
}

