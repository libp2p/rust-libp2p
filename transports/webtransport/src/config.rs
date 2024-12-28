use std::time::Duration;

use wtransport::config::TlsServerConfig;

use crate::certificate::{CertHash, Certificate};

#[derive(Clone)]
pub struct Config {
    /// Timeout for the initial handshake when establishing a connection.
    /// The actual timeout is the minimum of this and the [`Config::max_idle_timeout`].
    pub handshake_timeout: Duration,

    /// Maximum duration of inactivity in ms to accept before timing out the connection.
    pub max_idle_timeout: u32,

    /// Maximum number of incoming bidirectional streams that may be open
    /// concurrently by the remote peer.
    pub max_concurrent_stream_limit: u32,

    /// Max unacknowledged data in bytes that may be sent on a single stream.
    pub max_stream_data: u32,

    /// Max unacknowledged data in bytes that may be sent in total on all streams
    /// of a connection.
    pub max_connection_data: u32,

    /// Period of inactivity before sending a keep-alive packet.
    /// Must be set lower than the idle_timeout of both
    /// peers to be effective.
    ///
    /// See [`quinn::TransportConfig::keep_alive_interval`] for more
    /// info.
    pub keep_alive_interval: Duration,

    /// Libp2p identity of the node.
    pub keypair: libp2p_identity::Keypair,

    cert: Certificate,
}

impl Config {
    pub fn new(keypair: &libp2p_identity::Keypair, cert: Certificate) -> Self {
        Self {
            handshake_timeout: Duration::from_secs(5),
            max_idle_timeout: 10 * 1000,
            max_concurrent_stream_limit: 256,
            keep_alive_interval: Duration::from_secs(5),
            max_connection_data: 15_000_000,
            // Ensure that one stream is not consuming the whole connection.
            max_stream_data: 10_000_000,
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
    vec![
        b"libp2p".to_vec(),
        b"h3".to_vec(),
    ]
}
