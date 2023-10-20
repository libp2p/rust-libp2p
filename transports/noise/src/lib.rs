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
//! use libp2p_core::{Transport, upgrade, transport::MemoryTransport};
//! use libp2p_noise as noise;
//! use libp2p_identity as identity;
//!
//! # fn main() {
//! let id_keys = identity::Keypair::generate_ed25519();
//! let noise = noise::Config::new(&id_keys).unwrap();
//! let builder = MemoryTransport::default().upgrade(upgrade::Version::V1).authenticate(noise);
//! // let transport = builder.multiplex(...);
//! # }
//! ```
//!
//! [noise]: http://noiseprotocol.org/

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![allow(deprecated)] // Temporarily until we remove deprecated items.

mod io;
mod protocol;

pub use io::Output;

use crate::handshake::State;
use crate::io::handshake;
use crate::protocol::{noise_params_into_builder, AuthenticKeypair, Keypair, PARAMS_XX};
use futures::prelude::*;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use multiaddr::Protocol;
use multihash::Multihash;
use snow::params::NoiseParams;
use std::collections::HashSet;
use std::fmt::Write;
use std::pin::Pin;

/// The configuration for the noise handshake.
#[derive(Clone)]
pub struct Config {
    dh_keys: AuthenticKeypair,
    params: NoiseParams,
    webtransport_certhashes: Option<HashSet<Multihash<64>>>,

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
        let noise_keys = Keypair::new().into_authentic(identity)?;

        Ok(Self {
            dh_keys: noise_keys,
            params: PARAMS_XX.clone(),
            webtransport_certhashes: None,
            prologue: vec![],
        })
    }

    /// Set the noise prologue.
    pub fn with_prologue(mut self, prologue: Vec<u8>) -> Self {
        self.prologue = prologue;
        self
    }

    /// Set WebTransport certhashes extension.
    ///
    /// In case of initiator, these certhashes will be used to validate the ones reported by
    /// responder.
    ///
    /// In case of responder, these certhashes will be reported to initiator.
    pub fn with_webtransport_certhashes(mut self, certhashes: HashSet<Multihash<64>>) -> Self {
        self.webtransport_certhashes = Some(certhashes).filter(|h| !h.is_empty());
        self
    }

    fn into_responder<S>(self, socket: S) -> Result<State<S>, Error> {
        let session = noise_params_into_builder(
            self.params,
            &self.prologue,
            self.dh_keys.keypair.secret(),
            None,
        )
        .build_responder()?;

        let state = State::new(
            socket,
            session,
            self.dh_keys.identity,
            None,
            self.webtransport_certhashes,
        );

        Ok(state)
    }

    fn into_initiator<S>(self, socket: S) -> Result<State<S>, Error> {
        let session = noise_params_into_builder(
            self.params,
            &self.prologue,
            self.dh_keys.keypair.secret(),
            None,
        )
        .build_initiator()?;

        let state = State::new(
            socket,
            session,
            self.dh_keys.identity,
            None,
            self.webtransport_certhashes,
        );

        Ok(state)
    }
}

impl UpgradeInfo for Config {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once("/noise")
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

            let (pk, io) = state.finish()?;

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

            let (pk, io) = state.finish()?;

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
    #[error("failed to decode protobuf ")]
    InvalidPayload(#[from] DecodeError),
    #[error(transparent)]
    SigningError(#[from] libp2p_identity::SigningError),
    #[error("Expected WebTransport certhashes ({}) are not a subset of received ones ({})", certhashes_to_string(.0), certhashes_to_string(.1))]
    UnknownWebTransportCerthashes(HashSet<Multihash<64>>, HashSet<Multihash<64>>),
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct DecodeError(quick_protobuf::Error);

fn certhashes_to_string(certhashes: &HashSet<Multihash<64>>) -> String {
    let mut s = String::new();

    for hash in certhashes {
        write!(&mut s, "{}", Protocol::Certhash(*hash)).unwrap();
    }

    s
}
