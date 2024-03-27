// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

use quinn::{MtuDiscoveryConfig, TransportConfig, VarInt};
use std::{sync::Arc, time::Duration};
use libp2p_core::multihash::Multihash;
use libp2p_tls::certificate;
use crate::certificate_manager::ServerCertManager;

/// Config for the transport.
#[derive(Clone, Debug)]
pub struct Config {
    /// Timeout for the initial handshake when establishing a connection.
    /// The actual timeout is the minimum of this and the [`Config::max_idle_timeout`].
    pub handshake_timeout: Duration,
    /// Maximum duration of inactivity in ms to accept before timing out the connection.
    pub max_idle_timeout: u32,
    /// Period of inactivity before sending a keep-alive packet.
    /// Must be set lower than the idle_timeout of both
    /// peers to be effective.
    ///
    /// See [`quinn::TransportConfig::keep_alive_interval`] for more
    /// info.
    pub keep_alive_interval: Duration,
    /// Maximum number of incoming bidirectional streams that may be open
    /// concurrently by the remote peer.
    pub max_concurrent_stream_limit: u32,

    /// Max unacknowledged data in bytes that may be send on a single stream.
    pub max_stream_data: u32,

    /// Max unacknowledged data in bytes that may be send in total on all streams
    /// of a connection.
    pub max_connection_data: u32,

    /// Support QUIC version draft-29 for dialing and listening.
    ///
    /// Per default only QUIC Version 1 / [`libp2p_core::multiaddr::Protocol::QuicV1`]
    /// is supported.
    ///
    /// If support for draft-29 is enabled servers support draft-29 and version 1 on all
    /// QUIC listening addresses.
    /// As client the version is chosen based on the remote's address.
    pub support_draft_29: bool,

    // pub web_transport_config: WebTransportConfig,

    /// TLS client config for the inner [`quinn::ClientConfig`].
    client_tls_config: Arc<rustls::ClientConfig>,

    transport: Arc<TransportConfig>,

    endpoint_config: quinn::EndpointConfig,

    /// TLS server config manager for the inner [`quinn::ServerConfig`] and
    /// self-signed certificate hashes.
    cert_manager: ServerCertManager,

    /// Libp2p identity of the node.
    keypair: libp2p_identity::Keypair,

    /// Parameters governing MTU discovery. See [`MtuDiscoveryConfig`] for details.
    mtu_discovery_config: Option<MtuDiscoveryConfig>,
}

impl Config {
    /// Creates a new configuration object with default values.
    pub fn new(keypair: &libp2p_identity::Keypair) -> Self {
        let client_tls_config = Arc::new(libp2p_tls::make_client_config(keypair, None).unwrap());

        let max_concurrent_stream_limit = 256;
        let keep_alive_interval = Duration::from_secs(5);
        let max_idle_timeout = 10 * 1000;
        let max_stream_data = 10_000_000;
        let max_connection_data = 15_000_000;
        let mtu_discovery_config = Some(Default::default());
        let support_draft_29 = false;

        let mut transport = quinn::TransportConfig::default();
        // Disable uni-directional streams.
        transport.max_concurrent_uni_streams(0u32.into());
        transport.max_concurrent_bidi_streams(max_concurrent_stream_limit.into());
        // Disable datagrams.
        transport.datagram_receive_buffer_size(None);
        transport.keep_alive_interval(Some(keep_alive_interval.clone()));
        transport.max_idle_timeout(Some(VarInt::from_u32(max_idle_timeout).into()));
        transport.allow_spin(false);
        transport.stream_receive_window(max_stream_data.into());
        transport.receive_window(max_connection_data.into());
        transport.mtu_discovery_config(mtu_discovery_config.clone());

        let mut endpoint_config = keypair
            .derive_secret(b"libp2p quic stateless reset key")
            .map(|secret| {
                let reset_key = Arc::new(ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &secret));
                quinn::EndpointConfig::new(reset_key)
            })
            .unwrap_or_default();

        if !support_draft_29 {
            endpoint_config.supported_versions(vec![1]);
        }

        Self {
            keypair: keypair.clone(),
            client_tls_config,
            transport: Arc::new(transport),
            endpoint_config,
            cert_manager: ServerCertManager::new(keypair.clone()),
            support_draft_29,
            handshake_timeout: Duration::from_secs(5),
            max_idle_timeout,
            max_concurrent_stream_limit,
            keep_alive_interval,
            max_connection_data,
            // Ensure that one stream is not consuming the whole connection.
            max_stream_data,
            mtu_discovery_config,
        }
    }

    /// Disable MTU path discovery (it is enabled by default).
    pub fn disable_path_mtu_discovery(mut self) -> Self {
        self.mtu_discovery_config = None;
        self
    }

    pub fn server_quinn_config(&mut self
    ) -> Result<(quinn::ServerConfig, libp2p_noise::Config, Vec<Multihash<64>>), certificate::GenError> {
        let (server_tls_config, cert_hashes) = self.cert_manager.get_config()?;

        let mut server_config = quinn::ServerConfig::with_crypto(server_tls_config);
        server_config.transport = Arc::clone(&self.transport);
        // Disables connection migration.
        // Long-term this should be enabled, however we then need to handle address change
        // on connections in the `Connection`.
        server_config.migration(false);

        let mut noise = libp2p_noise::Config::new(&self.keypair)
            .expect("Gets noise config");
        noise = noise.with_webtransport_certhashes(
            cert_hashes.clone().into_iter().collect()
        );

        Ok((server_config, noise, cert_hashes))
    }

    pub fn endpoint_config(&self) -> quinn::EndpointConfig {
        self.endpoint_config.clone()
    }

    pub fn client_quinn_config(&self) -> quinn::ClientConfig {
        let mut client_config = quinn::ClientConfig::new(self.client_tls_config.clone());
        client_config.transport_config(Arc::clone(&self.transport));

        client_config
    }
}
