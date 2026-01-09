//! Propeller network behaviour implementation.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    task::{Context, Poll, Waker},
};

use libp2p_core::Endpoint;
use libp2p_identity::{Keypair, PeerId, PublicKey};
use libp2p_swarm::{
    behaviour::{ConnectionClosed, ConnectionEstablished, FromSwarm},
    ConnectionDenied, ConnectionId, NetworkBehaviour, NotifyHandler, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use tracing::debug;

use crate::{
    config::Config,
    handler::{Handler, HandlerIn, HandlerOut},
    message::{MessageId, PropellerMessage, Shred, ShredId, ShredIndex},
    signature,
    tree::{PropellerTree, PropellerTreeManager},
    types::{
        Event, PeerSetError, ReconstructionError, ShredPublishError,
        ShredSignatureVerificationError, ShredValidationError,
    },
    ValidationMode,
};

/// Determines the authenticity requirements for messages.
///
/// This controls how messages are signed and validated in the Propeller protocol.
#[derive(Clone)]
pub enum MessageAuthenticity {
    /// Message signing is enabled. The author will be the owner of the key.
    Signed(Keypair),
    /// Message signing is disabled.
    ///
    /// The specified [`PeerId`] will be used as the author of all published messages.
    Author(PeerId),
}

/// The Propeller network behaviour.
pub struct Behaviour {
    /// Configuration for this behaviour.
    config: Config,

    /// Events to be returned to the swarm.
    events: VecDeque<ToSwarm<Event, HandlerIn>>,

    /// Waker for the behaviour.
    waker: Option<Waker>,

    /// Dynamic tree manager for computing topology per-shred.
    tree_manager: PropellerTreeManager,

    /// Currently connected peers.
    connected_peers: HashSet<PeerId>,

    /// Message authenticity configuration for signing/verification.
    message_authenticity: MessageAuthenticity,

    /// Map of peer IDs to their public keys for signature verification.
    peer_public_keys: HashMap<PeerId, PublicKey>,

    /// Verified shreds organized by message id.
    verified_shreds: lru_time_cache::LruCache<(PeerId, MessageId), HashMap<ShredIndex, Shred>>,

    /// Cache of message ids for which messages have already been reconstructed and emitted.
    reconstructed_messages: lru_time_cache::LruCache<(PeerId, MessageId), ()>,
}

impl Behaviour {
    /// Create a new Propeller behaviour.
    pub fn new(message_authenticity: MessageAuthenticity, config: Config) -> Self {
        let local_peer_id = match &message_authenticity {
            MessageAuthenticity::Signed(keypair) => PeerId::from(keypair.public()),
            MessageAuthenticity::Author(peer_id) => *peer_id,
        };

        let reconstructed_messages =
            lru_time_cache::LruCache::with_expiry_duration(config.reconstructed_messages_ttl());
        let verified_shreds =
            lru_time_cache::LruCache::with_expiry_duration(config.verified_shreds_ttl());

        Self {
            tree_manager: PropellerTreeManager::new(local_peer_id, config.fanout()),
            config,
            waker: None,
            events: VecDeque::new(),
            connected_peers: HashSet::new(),
            message_authenticity,
            peer_public_keys: HashMap::new(),
            verified_shreds,
            reconstructed_messages,
        }
    }

    /// Add multiple peers with their weights for tree topology calculation.
    ///
    /// This method allows you to add multiple peers at once, each with an associated weight
    /// that determines their position in the dissemination tree. Higher weight peers are
    /// positioned closer to the root, making them more likely to receive messages earlier.
    pub fn set_peers(
        &mut self,
        peers: impl IntoIterator<Item = (PeerId, u64)>,
    ) -> Result<(), PeerSetError> {
        self.set_peers_and_optional_keys(
            peers
                .into_iter()
                .map(|(peer_id, weight)| (peer_id, weight, None)),
        )
    }

    pub fn set_peers_and_keys(
        &mut self,
        peers: impl IntoIterator<Item = (PeerId, u64, PublicKey)>,
    ) -> Result<(), PeerSetError> {
        self.set_peers_and_optional_keys(
            peers
                .into_iter()
                .map(|(peer_id, weight, public_key)| (peer_id, weight, Some(public_key))),
        )
    }

    pub fn set_peers_and_optional_keys(
        &mut self,
        peers: impl IntoIterator<Item = (PeerId, u64, Option<PublicKey>)>,
    ) -> Result<(), PeerSetError> {
        self.peer_public_keys.clear();
        self.tree_manager.clear();

        let mut peer_weights = HashMap::new();
        for (peer_id, weight, public_key) in peers {
            self.add_peer_with_key(peer_id, weight, public_key)?;
            peer_weights.insert(peer_id, weight);
        }
        self.tree_manager.update_nodes(&peer_weights)?;
        Ok(())
    }

    /// Add a peer with its weight and explicit public key for signature verification.
    fn add_peer_with_key(
        &mut self,
        peer_id: PeerId,
        weight: u64,
        public_key: Option<PublicKey>,
    ) -> Result<(), PeerSetError> {
        if let Some(public_key) = public_key {
            if signature::validate_public_key_matches_peer_id(&public_key, &peer_id) {
                self.peer_public_keys.insert(peer_id, public_key);
            } else {
                return Err(PeerSetError::InvalidPublicKey);
            }
        } else if let Some(extracted_key) = signature::try_extract_public_key_from_peer_id(&peer_id)
        {
            self.peer_public_keys.insert(peer_id, extracted_key);
        } else {
            return Err(PeerSetError::InvalidPublicKey);
        }
        debug!(peer=%peer_id, weight=%weight, "Added peer {} with weight {}", peer_id, weight);

        Ok(())
    }

    /// Get the number of peers this node knows about.
    pub fn peer_count(&self) -> usize {
        self.tree_manager.len()
    }

    /// Broadcast data as shreds
    /// The data will be split into fec_data_shreds equal parts.
    /// The data size must be divisible by fec_data_shreds.
    pub fn broadcast(
        &mut self,
        data: Vec<u8>,
        message_id: MessageId,
    ) -> Result<Vec<Shred>, ShredPublishError> {
        // Validate data size is divisible by number of data shreds
        let num_data_shreds = self.config.fec_data_shreds();
        if data.len() % num_data_shreds != 0 {
            return Err(ShredPublishError::InvalidDataSize);
        }

        // Create shreds from data
        let shreds = self.create_shreds_from_data(data, message_id)?;

        // Send shreds to root (if there are other peers)
        for shred in shreds.iter() {
            let shred_hash = shred.hash();
            let tree = self
                .tree_manager
                .build_tree(&self.tree_manager.get_local_peer_id(), &shred_hash)
                .map_err(ShredPublishError::TreeGenerationError)?;

            // Only send if there's a root (tree is not empty)
            if let Some(root) = tree.get_root() {
                self.broadcast_shred_to_peer(shred.clone(), root);
            }
        }

        Ok(shreds)
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Add multiple peers to the connected peers set for testing purposes.
    pub fn add_connected_peers_for_test(&mut self, peer_ids: Vec<PeerId>) {
        for peer_id in peer_ids {
            self.connected_peers.insert(peer_id);
        }
    }

    /// Verify the signature of a shred.
    fn verify_shred_signature(&self, shred: &Shred) -> Result<(), ShredSignatureVerificationError> {
        if self.config.validation_mode() == &ValidationMode::None {
            return Ok(());
        }

        // Get the signer's public key
        let Some(signer_public_key) = self.peer_public_keys.get(&shred.id.publisher) else {
            return Err(ShredSignatureVerificationError::NoPublicKeyAvailable(
                shred.id.publisher,
            ));
        };

        // Use the extracted signature verification function
        signature::verify_shred_signature(shred, signer_public_key)
    }

    /// Sign a shred using our keypair (if available).
    fn sign_shred(&self, shred: &Shred) -> Result<Vec<u8>, ShredPublishError> {
        signature::sign_shred(shred, &self.message_authenticity)
    }

    /// Validate a received shred from a peer.
    /// Returns Ok(tree) if validation passes, or Err(ShredVerificationError) if validation fails.
    pub fn validate_shred(
        &mut self,
        sender: PeerId,
        shred: &Shred,
    ) -> Result<PropellerTree, ShredValidationError> {
        if let Some(message_shreds) = self
            .verified_shreds
            .get(&(shred.id.publisher, shred.id.message_id))
        {
            if message_shreds.contains_key(&shred.id.index) {
                return Err(ShredValidationError::DuplicateShred);
            }
        }

        if let Err(e) = self.verify_shred_signature(shred) {
            return Err(ShredValidationError::SignatureVerificationFailed(e));
        }

        if shred.id.publisher == self.tree_manager.get_local_peer_id() {
            return Err(ShredValidationError::ReceivedPublishedShred);
        }

        let shred_hash = shred.hash();
        let tree = self
            .tree_manager
            .build_tree(&shred.id.publisher, &shred_hash)
            .unwrap();

        // Validate sender is either:
        // 1. The publisher (if we are root)
        // 2. Our parent in the tree (normal case)
        let is_valid_sender = if tree.is_local_root() {
            // Root receives from publisher
            sender == shred.id.publisher
        } else {
            // Non-root receives from parent
            tree.get_parent() == Some(sender)
        };

        if !is_valid_sender {
            let expected_sender = if tree.is_local_root() {
                shred.id.publisher
            } else {
                tree.get_parent().unwrap()
            };
            return Err(ShredValidationError::ParentVerificationFailed { expected_sender });
        }

        Ok(tree)
    }

    /// Handle a received shred from a peer with full verification.
    fn handle_received_shred(&mut self, sender: PeerId, shred: Shred) {
        let tree = match self.validate_shred(sender, &shred) {
            Ok(tree) => tree,
            Err(error) => {
                self.emit_event(Event::ShredValidationFailed {
                    sender,
                    shred_id: shred.id,
                    error,
                });
                return;
            }
        };
        let publisher = shred.id.publisher;

        let key = (shred.id.publisher, shred.id.message_id);
        let message_shreds = self.verified_shreds.entry(key).or_insert_with(HashMap::new);
        message_shreds.insert(shred.id.index, shred.clone());

        // Emit event for the verified shred
        if self.config.emit_shred_received_events() {
            self.emit_event(Event::ShredReceived {
                sender,
                shred: shred.clone(),
            });
        }

        // Forward the shred to our children in the tree
        let message_id = shred.id.message_id;
        self.broadcast_shred_to_children_in_tree(shred, &tree);

        // Check if we have enough shreds to reconstruct the message
        self.try_reconstruct_message(publisher, message_id);
    }

    /// Try to reconstruct the original message from verified shreds for a given message id.
    fn try_reconstruct_message(&mut self, publisher: PeerId, message_id: MessageId) {
        // Check if we've already reconstructed and emitted this message
        if self
            .reconstructed_messages
            .contains_key(&(publisher, message_id))
        {
            return;
        }

        let message_shreds = self.verified_shreds.get(&(publisher, message_id)).unwrap();

        let expected_shreds = self.config.fec_data_shreds();
        if message_shreds.len() < expected_shreds {
            return;
        }

        // Collect all shreds and sort by index to ensure correct order
        // Clone the shreds to avoid borrowing issues
        let mut shreds: Vec<_> = message_shreds.values().cloned().collect();
        shreds.sort_by_key(|s| s.id.index);
        let shred_refs: Vec<_> = shreds.iter().collect();

        // Use Reed-Solomon error correction to reconstruct the message
        match self.reconstruct_message_from_shreds(&shred_refs) {
            Ok(reconstructed_data) => {
                // Mark this message id as reconstructed to prevent duplicate events
                self.reconstructed_messages
                    .insert((publisher, message_id), ());

                // Emit the reconstructed message event
                self.emit_event(Event::MessageReceived {
                    publisher,
                    message_id,
                    data: reconstructed_data,
                });
            }
            Err(e) => {
                self.emit_event(Event::MessageReconstructionFailed {
                    message_id,
                    publisher,
                    error: e,
                });
            }
        }
    }

    /// Create shreds from raw data.
    /// Data will be split into num_data_shreds equal parts.
    pub fn create_shreds_from_data(
        &mut self,
        data: Vec<u8>,
        message_id: MessageId,
    ) -> Result<Vec<Shred>, ShredPublishError> {
        let num_data_shreds = self.config.fec_data_shreds();
        let shred_size = data.len() / num_data_shreds;
        let mut shreds = Vec::new();

        // Data should be divisible by num_data_shreds due to validation in broadcast()
        assert_eq!(data.len() % num_data_shreds, 0);

        // Split data into exactly num_data_shreds shreds of equal size
        for (index, chunk) in data.chunks_exact(shred_size).enumerate() {
            let shred_id = ShredId {
                message_id,
                index: index as ShredIndex,
                publisher: self.tree_manager.get_local_peer_id(),
            };

            // Create the shred with empty signature first
            let mut shred = Shred {
                id: shred_id,
                shard: chunk.to_vec(),
                signature: Vec::new(),
            };

            // Sign the shred if we have signing capability
            let signature = self.sign_shred(&shred)?;
            shred.signature = signature;

            shreds.push(shred);
        }

        // Generate coding shreds
        let coding_shreds = self.generate_coding_shreds(&shreds)?;
        shreds.extend(coding_shreds);

        Ok(shreds)
    }

    /// Generate coding shreds using Reed-Solomon encoding.
    fn generate_coding_shreds(
        &self,
        data_shreds: &[Shred],
    ) -> Result<Vec<Shred>, ShredPublishError> {
        use reed_solomon_simd::ReedSolomonEncoder;

        let data_count = self.config.fec_data_shreds();
        assert_eq!(data_count, data_shreds.len());
        let coding_count = self.config.fec_coding_shreds();

        // Get shred size from the first data shred (all data shreds should be the same size)
        let shred_size = data_shreds
            .first()
            .ok_or(ShredPublishError::ErasureEncodingFailed(
                "No data shreds".to_string(),
            ))?
            .shard
            .len();

        // Get the message id from the first data shred
        let message_id = data_shreds[0].id.message_id;

        // Create Reed-Solomon encoder
        let mut encoder =
            ReedSolomonEncoder::new(data_count, coding_count, shred_size).map_err(|e| {
                ShredPublishError::ErasureEncodingFailed(format!(
                    "Failed to create Reed-Solomon encoder: {}",
                    e
                ))
            })?;

        // Add data shreds (all should be the same size)
        for shred in data_shreds.iter().take(data_count) {
            encoder.add_original_shard(&shred.shard).map_err(|e| {
                ShredPublishError::ErasureEncodingFailed(format!("Failed to add data shred: {}", e))
            })?;
        }

        // Perform Reed-Solomon encoding
        let result = encoder.encode().map_err(|e| {
            ShredPublishError::ErasureEncodingFailed(format!("Failed to encode: {}", e))
        })?;

        // Create coding shreds from the recovery data
        let mut coding_shreds = Vec::with_capacity(coding_count);
        for (i, recovery_shard) in result.recovery_iter().enumerate() {
            let shred_id = ShredId {
                message_id,
                index: (data_count + i) as ShredIndex, // Coding shreds start after data shreds
                publisher: self.tree_manager.get_local_peer_id(),
            };

            // Create the coding shred with empty signature first
            let mut shred = Shred {
                id: shred_id,
                shard: recovery_shard.to_vec(),
                signature: Vec::new(),
            };

            // Sign the coding shred
            let signature = self.sign_shred(&shred)?;
            shred.signature = signature;

            coding_shreds.push(shred);
        }

        Ok(coding_shreds)
    }

    /// Reconstruct the original message from available shreds using Reed-Solomon error correction.
    fn reconstruct_message_from_shreds(
        &self,
        shreds: &[&Shred],
    ) -> Result<Vec<u8>, ReconstructionError> {
        use reed_solomon_simd::ReedSolomonDecoder;

        let data_count = self.config.fec_data_shreds();
        let coding_count = self.config.fec_coding_shreds();

        // Get shred size from the first available shred
        let shred_size = shreds
            .first()
            .ok_or(ReconstructionError::ErasureDecodingFailed(
                "No shreds".to_string(),
            ))?
            .shard
            .len();

        // Create Reed-Solomon decoder
        let mut decoder =
            ReedSolomonDecoder::new(data_count, coding_count, shred_size).map_err(|e| {
                ReconstructionError::ErasureDecodingFailed(format!(
                    "Failed to create Reed-Solomon decoder: {}",
                    e
                ))
            })?;

        // Create a mapping of shred index to shard for efficient lookup
        let mut shred_map = std::collections::HashMap::new();
        for shred in shreds {
            shred_map.insert(shred.id.index, &shred.shard);
        }

        // Add available shreds to decoder in index order
        for index in 0..(data_count + coding_count) {
            if let Some(shred_data) = shred_map.get(&(index as u32)) {
                if index < data_count {
                    decoder.add_original_shard(index, shred_data).map_err(|e| {
                        ReconstructionError::ErasureDecodingFailed(format!(
                            "Failed to add original shard: {}",
                            e
                        ))
                    })?;
                } else {
                    decoder
                        .add_recovery_shard(index - data_count, shred_data)
                        .map_err(|e| {
                            ReconstructionError::ErasureDecodingFailed(format!(
                                "Failed to add coding shard: {}",
                                e
                            ))
                        })?;
                }
            }
        }

        // Perform Reed-Solomon decoding to reconstruct missing data shreds
        let result = decoder.decode().map_err(|e| {
            ReconstructionError::ErasureDecodingFailed(format!("Failed to decode: {}", e))
        })?;

        // Combine the reconstructed data shreds to form the original message
        let mut reconstructed_data = Vec::new();
        for index in 0..data_count {
            if let Some(shred_shard) = shred_map.get(&(index as u32)) {
                // We have the original shred shard
                reconstructed_data.extend_from_slice(shred_shard);
            } else {
                // We need to get the restored shard from Reed-Solomon
                if let Some(restored_data) = result
                    .restored_original_iter()
                    .find(|(restored_index, _)| *restored_index == index)
                    .map(|(_, data)| data)
                {
                    reconstructed_data.extend_from_slice(restored_data);
                } else {
                    return Err(ReconstructionError::ErasureDecodingFailed(format!(
                        "Missing data shard at index {} and no restored data available",
                        index
                    )));
                }
            }
        }

        debug!(
            "Reconstructed message of {} bytes from {} available shreds using Reed-Solomon",
            reconstructed_data.len(),
            shreds.len()
        );

        Ok(reconstructed_data)
    }

    fn broadcast_shred_to_peer(&mut self, shred: Shred, peer: PeerId) {
        let message = PropellerMessage { shred };
        self.emit_handler_event(peer, HandlerIn::SendMessage(message));
    }

    /// Broadcast a single shred to appropriate peers in the propeller tree.
    fn broadcast_shred_to_children_in_tree(&mut self, shred: Shred, tree: &PropellerTree) {
        // Get broadcast peer (root node) for this shred
        let children = tree.get_children();
        for child in children {
            self.broadcast_shred_to_peer(shred.clone(), child);
        }
    }

    fn emit_event(&mut self, event: Event) {
        self.events.push_back(ToSwarm::GenerateEvent(event));
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    fn emit_handler_event(&mut self, peer_id: PeerId, event: HandlerIn) {
        // Check if we're connected to this peer before trying to send
        if !self.connected_peers.contains(&peer_id) {
            // Emit a send failed event immediately if not connected
            self.emit_event(Event::ShredSendFailed {
                sent_from: None,
                sent_to: Some(peer_id),
                error: ShredPublishError::NotConnectedToPeer(peer_id),
            });
            return;
        }

        self.events.push_back(ToSwarm::NotifyHandler {
            peer_id,
            handler: NotifyHandler::Any,
            event,
        });
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(
            self.config.stream_protocol().clone(),
            self.config.max_shred_size(),
            self.config.substream_timeout(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &libp2p_core::Multiaddr,
        _role_override: Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(
            self.config.stream_protocol().clone(),
            self.config.max_shred_size(),
            self.config.substream_timeout(),
        ))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id: _,
                endpoint: _,
                failed_addresses: _,
                other_established: _,
            }) => {
                self.connected_peers.insert(peer_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id: _,
                endpoint: _,
                remaining_established,
                cause: _,
            }) => {
                if remaining_established == 0 {
                    self.connected_peers.remove(&peer_id);
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {
            HandlerOut::Message(message) => {
                self.handle_received_shred(peer_id, message.shred);
            }
            HandlerOut::SendError(error) => {
                self.emit_event(Event::ShredSendFailed {
                    sent_from: None,
                    sent_to: Some(peer_id),
                    error: ShredPublishError::HandlerError(error),
                });
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.waker = Some(cx.waker().clone());

        // Return any pending events
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
