use std::time::Duration;

use quinn::{MtuDiscoveryConfig, VarInt};
use wtransport::config::{QuicTransportConfig, TlsServerConfig};

use crate::certificate::{CertHash, Certificate};

pub struct Config {
    pub max_idle_timeout: u32,
    pub max_concurrent_stream_limit: u32,
    pub keep_alive_interval: Duration,
    pub max_connection_data: u32,
    pub max_stream_data: u32,
    pub mtu_discovery_config: MtuDiscoveryConfig,
    /// Timeout for the initial handshake when establishing a connection.
    /// The actual timeout is the minimum of this and the [`Config::max_idle_timeout`].
    pub handshake_timeout: Duration,
    /// Libp2p identity of the node.
    pub keypair: libp2p_identity::Keypair,

    cert: Certificate,
}

impl Config {
    pub fn new(keypair: &libp2p_identity::Keypair, cert: Certificate) -> Self {
        let max_idle_timeout = 30 * 1000;
        let max_concurrent_stream_limit = 256;
        let keep_alive_interval = Duration::from_secs(5);
        let max_connection_data = 15_000_000;
        // Ensure that one stream is not consuming the whole connection.
        let max_stream_data = 10_000_000;
        let mtu_discovery_config = Default::default();

        Self {
            max_idle_timeout,
            max_concurrent_stream_limit,
            keep_alive_interval,
            max_connection_data,
            max_stream_data,
            mtu_discovery_config,
            handshake_timeout: Duration::from_secs(5),
            keypair: keypair.clone(),
            cert,
        }
    }

    pub fn server_tls_config(&self) -> TlsServerConfig {
        libp2p_tls::make_webtransport_server_config(
            self.cert.der.clone(),
            &self.cert.private_key_der,
            alpn_protocols(),
        )
    }

    pub fn get_quic_transport_config(&self) -> QuicTransportConfig {
        let mut res = QuicTransportConfig::default();

        res.max_concurrent_uni_streams(100u32.into());
        res.max_concurrent_bidi_streams(self.max_concurrent_stream_limit.into());
        res.datagram_receive_buffer_size(None);
        res.keep_alive_interval(Some(self.keep_alive_interval.clone()));
        res.max_idle_timeout(Some(VarInt::from_u32(self.max_idle_timeout).into()));
        res.allow_spin(true);
        res.stream_receive_window(self.max_stream_data.into());
        res.receive_window(self.max_connection_data.into());
        res.mtu_discovery_config(Some(self.mtu_discovery_config.clone()));

        res
    }

    pub fn cert_hashes(&self) -> Vec<CertHash> {
        vec![self.cert.cert_hash()]
    }
}

fn alpn_protocols() -> Vec<Vec<u8>> {
    vec![b"libp2p".to_vec(), b"h3".to_vec()]
}
