use quinn::{TransportConfig, VarInt};
use std::sync::Arc;
use std::time::Duration;
use wtransport::config::TlsServerConfig;

use crate::certificate::{CertHash, Certificate};

pub struct Config {
    pub quic_transport_config: Arc<TransportConfig>,
    /// Timeout for the initial handshake when establishing a connection.
    /// The actual timeout is the minimum of this and the [`Config::max_idle_timeout`].
    pub handshake_timeout: Duration,
    /// Libp2p identity of the node.
    pub keypair: libp2p_identity::Keypair,

    cert: Certificate,
}

impl Config {
    pub fn new(keypair: &libp2p_identity::Keypair, cert: Certificate) -> Self {
        let max_idle_timeout = 10 * 1000;
        let max_concurrent_stream_limit = 256;
        let keep_alive_interval = Duration::from_secs(5);
        let max_connection_data = 15_000_000;
        // Ensure that one stream is not consuming the whole connection.
        let max_stream_data = 10_000_000;
        let mtu_discovery_config = Some(Default::default());

        let mut transport = quinn::TransportConfig::default();
        // Disable uni-directional streams.
        transport.max_concurrent_uni_streams(0u32.into());
        transport.max_concurrent_bidi_streams(max_concurrent_stream_limit.into());
        // Disable datagrams.
        transport.datagram_receive_buffer_size(None);
        transport.keep_alive_interval(Some(keep_alive_interval));
        transport.max_idle_timeout(Some(VarInt::from_u32(max_idle_timeout).into()));
        transport.allow_spin(false);
        transport.stream_receive_window(max_stream_data.into());
        transport.receive_window(max_connection_data.into());
        transport.mtu_discovery_config(mtu_discovery_config);
        let transport = Arc::new(transport);

        Self {
            quic_transport_config: transport,
            handshake_timeout: Duration::from_secs(5),
            keypair: keypair.clone(),
            cert,
        }
    }

    pub fn server_tls_config(&self) -> TlsServerConfig {
        libp2p_tls::make_webtransport_server_config(
            &self.cert.der,
            &self.cert.private_key_der,
            alpn_protocols(),
        )
    }

    pub fn cert_hashes(&self) -> Vec<CertHash> {
        vec![self.cert.cert_hash()]
    }
}

fn alpn_protocols() -> Vec<Vec<u8>> {
    vec![b"libp2p".to_vec(), b"h3".to_vec()]
}
