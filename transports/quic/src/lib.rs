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
mod tls;
mod transport;

use crate::crypto::TlsCrypto;
pub use crate::muxer::{QuicMuxer, QuicMuxerError};
pub use crate::transport::{QuicDial, QuicTransport};
pub use quinn_proto::{ConfigError, ConnectError, ConnectionError, TransportConfig};

use libp2p_core::identity::Keypair;
use libp2p_core::transport::TransportError;
use libp2p_core::Multiaddr;
use std::sync::Arc;
use thiserror::Error;

/// Quic configuration.
pub struct QuicConfig {
    pub keypair: Keypair,
    pub transport: TransportConfig,
    pub keylogger: Option<Arc<dyn rustls::KeyLog>>,
}

impl std::fmt::Debug for QuicConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("QuicConfig")
            .field("keypair", &self.keypair.public())
            .field("transport", &self.transport)
            .finish()
    }
}

impl QuicConfig {
    /// Creates a new config from a keypair.
    pub fn new(keypair: Keypair) -> Self {
        Self {
            keypair,
            transport: TransportConfig::default(),
            keylogger: None,
        }
    }

    /// Enable keylogging.
    pub fn enable_keylogger(&mut self) -> &mut Self {
        self.keylogger = Some(TlsCrypto::keylogger());
        self
    }

    /// Spawns a new endpoint.
    pub async fn listen_on(
        self,
        addr: Multiaddr,
    ) -> Result<QuicTransport, TransportError<QuicError>> {
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
