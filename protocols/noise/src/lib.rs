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

mod error;
mod io;
mod keys;
mod util;

pub mod ik;
pub mod ix;
pub mod xx;

pub use error::NoiseError;
pub use io::NoiseOutput;
pub use keys::{Curve25519, PublicKey, SecretKey, Keypair};

use ik::IK;
use ix::IX;
use libp2p_core::{UpgradeInfo, InboundUpgrade, OutboundUpgrade};
use lazy_static::lazy_static;
use snow;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};
use xx::XX;

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

#[derive(Clone)]
pub struct NoiseConfig<P, R = ()> {
    keypair: Arc<Keypair<Curve25519>>,
    params: snow::params::NoiseParams,
    remote: R,
    _marker: std::marker::PhantomData<P>
}

impl NoiseConfig<IX> {
    pub fn ix(kp: Keypair<Curve25519>) -> Self {
        NoiseConfig {
            keypair: Arc::new(kp),
            params: PARAMS_IX.clone(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl NoiseConfig<XX> {
    pub fn xx(kp: Keypair<Curve25519>) -> Self {
        NoiseConfig {
            keypair: Arc::new(kp),
            params: PARAMS_XX.clone(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl NoiseConfig<IK> {
    pub fn ik_listener(kp: Keypair<Curve25519>) -> Self {
        NoiseConfig {
            keypair: Arc::new(kp),
            params: PARAMS_IK.clone(),
            remote: (),
            _marker: std::marker::PhantomData
        }
    }
}

impl NoiseConfig<IK, PublicKey<Curve25519>> {
    pub fn ik_dialer(kp: Keypair<Curve25519>, remote: PublicKey<Curve25519>) -> Self {
        NoiseConfig {
            keypair: Arc::new(kp),
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
        std::iter::once(b"/noise/ix/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for NoiseConfig<IX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = ix::NoiseInboundFuture<T>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        ix::NoiseInboundFuture::new(socket, self)
    }
}

impl<T> OutboundUpgrade<T> for NoiseConfig<IX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = ix::NoiseOutboundFuture<T>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        ix::NoiseOutboundFuture::new(socket, self)
    }
}

// Handshake pattern XX /////////////////////////////////////////////////////

impl UpgradeInfo for NoiseConfig<XX> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/xx/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for NoiseConfig<XX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = xx::NoiseInboundFuture<T>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        xx::NoiseInboundFuture::new(socket, self)
    }
}

impl<T> OutboundUpgrade<T> for NoiseConfig<XX>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = xx::NoiseOutboundFuture<T>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        xx::NoiseOutboundFuture::new(socket, self)
    }
}

// Handshake pattern IK /////////////////////////////////////////////////////

impl UpgradeInfo for NoiseConfig<IK> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ik/0.1.0")
    }
}

impl UpgradeInfo for NoiseConfig<IK, PublicKey<Curve25519>> {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/noise/ik/0.1.0")
    }
}

impl<T> InboundUpgrade<T> for NoiseConfig<IK>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = ik::NoiseInboundFuture<T>;

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        ik::NoiseInboundFuture::new(socket, self)
    }
}

impl<T> OutboundUpgrade<T> for NoiseConfig<IK, PublicKey<Curve25519>>
where
    T: AsyncRead + AsyncWrite
{
    type Output = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;
    type Future = ik::NoiseOutboundFuture<T>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        ik::NoiseOutboundFuture::new(socket, self)
    }
}

