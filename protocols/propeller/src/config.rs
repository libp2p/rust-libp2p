//! Propeller protocol configuration.

use std::time::Duration;

use libp2p_swarm::StreamProtocol;

/// The types of message validation that can be employed by Propeller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationMode {
    /// This is the default setting. This requires all messages to have valid signatures
    /// and the message author to be present and valid.
    Strict,
    /// This setting does not check the author or signature fields of incoming messages.
    /// If these fields contain data, they are simply ignored.
    ///
    /// NOTE: This setting will consider messages with invalid signatures as valid messages.
    None,
}

/// Configuration for the Propeller protocol.
#[derive(Clone, Debug)]
pub struct Config {
    /// Number of peers each node forwards shreds to in the tree (default: 8).
    fanout: usize,
    /// Number of data shreds in a FEC set (default: 32).
    fec_data_shreds: usize,
    /// Number of coding shreds in a FEC set (default: 32).
    fec_coding_shreds: usize,
    /// Time to keep verified shreds in cache before eviction.
    verified_shreds_ttl: Duration,
    /// Time to keep reconstructed messages in cache to prevent duplicates.
    reconstructed_messages_ttl: Duration,
    /// Validation mode for incoming messages.
    validation_mode: ValidationMode,
    /// Stream protocol for the Propeller protocol.
    /// default is "/propeller/1.0.0"
    stream_protocol: StreamProtocol,
    /// Emit shred received events.
    emit_shred_received_events: bool,
    /// Maximum shred size in bytes
    max_shred_size: usize,
    /// Timeout for substream upgrades
    substream_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fanout: 8,
            fec_data_shreds: 32,
            fec_coding_shreds: 32,
            verified_shreds_ttl: Duration::from_secs(20),
            reconstructed_messages_ttl: Duration::from_secs(20),
            validation_mode: ValidationMode::Strict,
            stream_protocol: StreamProtocol::new("/propeller/1.0.0"),
            emit_shred_received_events: false,
            max_shred_size: 1 << 20,
            substream_timeout: Duration::from_secs(30),
        }
    }
}

impl Config {
    /// Number of peers each node forwards shreds to in the tree.
    pub fn fanout(&self) -> usize {
        self.fanout
    }

    /// Number of data shreds in a FEC set.
    pub fn fec_data_shreds(&self) -> usize {
        self.fec_data_shreds
    }

    /// Number of coding shreds in a FEC set.
    pub fn fec_coding_shreds(&self) -> usize {
        self.fec_coding_shreds
    }

    /// Time to keep verified shreds in cache before eviction.
    pub fn verified_shreds_ttl(&self) -> Duration {
        self.verified_shreds_ttl
    }

    /// Time to keep reconstructed messages in cache to prevent duplicates.
    pub fn reconstructed_messages_ttl(&self) -> Duration {
        self.reconstructed_messages_ttl
    }

    /// Get the validation mode for incoming messages.
    pub fn validation_mode(&self) -> &ValidationMode {
        &self.validation_mode
    }

    /// Get the stream protocol for the Propeller protocol.
    pub fn stream_protocol(&self) -> &StreamProtocol {
        &self.stream_protocol
    }

    /// Get the emit shred received events flag.
    pub fn emit_shred_received_events(&self) -> bool {
        self.emit_shred_received_events
    }

    /// Maximum shred size in bytes.
    pub fn max_shred_size(&self) -> usize {
        self.max_shred_size
    }

    /// Timeout for substream upgrades.
    pub fn substream_timeout(&self) -> Duration {
        self.substream_timeout
    }
}

/// Builder for Propeller configuration.
#[derive(Debug, Default)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Set the fanout (number of peers each node forwards shreds to).
    pub fn fanout(mut self, fanout: usize) -> Self {
        self.config.fanout = fanout;
        self
    }

    /// Set the number of data shreds in a FEC set.
    pub fn fec_data_shreds(mut self, count: usize) -> Self {
        self.config.fec_data_shreds = count;
        self
    }

    /// Set the number of coding shreds in a FEC set.
    pub fn fec_coding_shreds(mut self, count: usize) -> Self {
        self.config.fec_coding_shreds = count;
        self
    }

    /// Set the verified shreds TTL.
    pub fn verified_shreds_ttl(mut self, ttl: Duration) -> Self {
        self.config.verified_shreds_ttl = ttl;
        self
    }

    /// Set the reconstructed messages TTL.
    pub fn reconstructed_messages_ttl(mut self, ttl: Duration) -> Self {
        self.config.reconstructed_messages_ttl = ttl;
        self
    }

    /// Set the validation mode for incoming messages.
    pub fn validation_mode(mut self, validation_mode: ValidationMode) -> Self {
        self.config.validation_mode = validation_mode;
        self
    }

    /// Set the emit shred received events flag.
    pub fn emit_shred_received_events(mut self, emit_shred_received_events: bool) -> Self {
        self.config.emit_shred_received_events = emit_shred_received_events;
        self
    }

    /// Set the maximum shred size in bytes.
    pub fn max_shred_size(mut self, max_shred_size: usize) -> Self {
        self.config.max_shred_size = max_shred_size;
        self
    }

    /// Set the timeout for substream upgrades.
    pub fn substream_timeout(mut self, timeout: Duration) -> Self {
        self.config.substream_timeout = timeout;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> Config {
        self.config
    }
}

impl Config {
    /// Create a new configuration builder.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
}
