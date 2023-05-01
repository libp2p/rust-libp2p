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
use crate::protocol::x25519_spec::PARAMS_XX;
use futures::prelude::*;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use std::fmt;
use std::fmt::Formatter;
use std::pin::Pin;

/// The configuration for the noise handshake.
#[derive(Clone)]
pub struct Config {
    dh_keys: AuthenticKeypair<X25519Spec>,
    params: ProtocolParams,

    /// Prologue to use in the noise handshake.
    ///
    /// The prologue can contain arbitrary data that will be hashed into the noise handshake.
    /// For the handshake to succeed, both parties must set the same prologue.
    ///
    /// For further information, see <https://noiseprotocol.org/noise.html#prologue>.
    prologue: Vec<u8>,
}

impl Config {
    /// Construct a new configuration for the noise handshake using the XX handshake pattern.

    pub fn new(identity: &identity::Keypair) -> Result<Self, Error> {
        let noise_keys = Keypair::<X25519Spec>::new().into_authentic(identity)?;

        Ok(Self {
            dh_keys: noise_keys,
            params: PARAMS_XX.clone(),
            prologue: vec![],
        })
    }

    /// Set the noise prologue.

    pub fn with_prologue(mut self, prologue: Vec<u8>) -> Self {
        self.prologue = prologue;

        self
    }

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

    fn upgrade_inbound(self, socket: T, _: Self::Info) -> Self::Future {
        async move {
            let mut state = self.into_responder(socket)?;

            handshake::recv_empty(&mut state).await?;
            handshake::send_identity(&mut state).await?;
            handshake::recv_identity(&mut state).await?;

            let (remote, io) = state.finish()?;

            let pk = match remote {
                RemoteIdentity::IdentityKey(pk) => pk,
                _ => return Err(Error::AuthenticationFailed),
            };

            Ok((pk.to_peer_id(), io))
        }
        .boxed()
    }
}

impl<T> OutboundUpgrade<T> for Config
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: T, _: Self::Info) -> Self::Future {
        async move {
            let mut state = self.into_initiator(socket)?;

            handshake::send_empty(&mut state).await?;
            handshake::recv_identity(&mut state).await?;
            handshake::send_identity(&mut state).await?;

            let (remote, io) = state.finish()?;

            let pk = match remote {
                RemoteIdentity::IdentityKey(pk) => pk,
                _ => return Err(Error::AuthenticationFailed),
            };

            Ok((pk.to_peer_id(), io))
        }
        .boxed()
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
