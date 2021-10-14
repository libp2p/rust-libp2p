// Copyright 2021 David Craven.
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

mod crypto;
mod endpoint;
mod muxer;
#[cfg(feature = "tls")]
mod tls;
mod transport;

pub use crate::crypto::Crypto;
#[cfg(feature = "noise")]
pub use crate::crypto::NoiseCrypto;
#[cfg(feature = "tls")]
pub use crate::crypto::TlsCrypto;
pub use crate::crypto::ToLibp2p;
pub use crate::muxer::{QuicMuxer, QuicMuxerError};
pub use crate::transport::{QuicDial, QuicTransport};
#[cfg(feature = "noise")]
pub use quinn_noise::{KeyLog, KeyLogFile};
pub use quinn_proto::{ConfigError, ConnectError, ConnectionError, TransportConfig};

use libp2p_core::transport::TransportError;
use libp2p_core::Multiaddr;
use quinn_proto::crypto::Session;
use thiserror::Error;

/// Quic configuration.
pub struct QuicConfig<C: Crypto> {
    pub keypair: C::Keypair,
    pub psk: Option<[u8; 32]>,
    pub transport: TransportConfig,
    pub keylogger: Option<C::Keylogger>,
}

impl<C: Crypto> std::fmt::Debug for QuicConfig<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("QuicConfig")
            .field("keypair", &self.keypair.to_public())
            .field("psk", &self.psk)
            .field("transport", &self.transport)
            .finish()
    }
}

impl<C: Crypto> QuicConfig<C>
where
    <C::Session as Session>::ClientConfig: Send + Unpin,
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    /// Creates a new config from a keypair.
    pub fn new(keypair: C::Keypair) -> Self {
        Self {
            keypair,
            psk: None,
            transport: TransportConfig::default(),
            keylogger: None,
        }
    }

    /// Enable keylogging.
    pub fn enable_keylogger(&mut self) -> &mut Self {
        self.keylogger = Some(C::keylogger());
        self
    }

    /// Spawns a new endpoint.
    pub async fn listen_on(
        self,
        addr: Multiaddr,
    ) -> Result<QuicTransport<C>, TransportError<QuicError>> {
        QuicTransport::new(self, addr).await
    }
}

#[derive(Debug, Error)]
pub enum QuicError {
    #[error("{0}")]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Connect(#[from] ConnectError),
    #[error("{0}")]
    Muxer(#[from] QuicMuxerError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("a `StreamMuxerEvent` was generated before the handshake was complete.")]
    UpgradeError,
}
