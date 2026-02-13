use std::collections::BTreeSet;

use hashlink::LinkedHashMap;
use libp2p_core::PeerId;
use libp2p_swarm::{ConnectionId, NetworkBehaviour};

use crate::{
    handler::HandlerEvent,
    partial_messages::{
        Metadata, Partial, PartialAction, PartialError, PublishAction, ReceivedAction, State,
    },
    queue::Queue,
    rpc_proto::proto,
    types::{ControlAction, Extensions, PeerDetails, PeerKind, RpcIn, RpcOut, SubscriptionOpts},
    Behaviour, ConfigBuilder, MessageAuthenticity, TopicHash, ValidationMode,
};

#[test]
fn test_extensions_message_creation() {
    let extensions_rpc = RpcOut::Extensions(Extensions {
        test_extension: Some(true),
        partial_messages: None,
    });
    let proto_rpc: proto::RPC = extensions_rpc.into();

    assert!(proto_rpc.control.is_some());
    let control = proto_rpc.control.unwrap();
    assert!(control.extensions.is_some());
    let test_extension = control.extensions.unwrap().testExtension.unwrap();
    assert!(test_extension);
    assert!(control.ihave.is_empty());
    assert!(control.iwant.is_empty());
    assert!(control.graft.is_empty());
    assert!(control.prune.is_empty());
    assert!(control.idontwant.is_empty());
}

#[test]
fn test_handle_extensions_message() {
    let mut gs: Behaviour = Behaviour::new(
        MessageAuthenticity::Anonymous,
        ConfigBuilder::default()
            .validation_mode(ValidationMode::None)
            .build()
            .unwrap(),
    )
    .unwrap();

    let peer_id = PeerId::random();
    let messages = Queue::new(gs.config.connection_handler_queue_len());

    // Add peer without extensions
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_3,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: BTreeSet::new(),
            messages,
            dont_send: LinkedHashMap::new(),
            extensions: None,
        },
    );

    // Simulate receiving extensions message
    let extensions = Extensions {
        test_extension: Some(false),
        partial_messages: None,
    };
    gs.handle_extensions(&peer_id, extensions);

    // Verify extensions were stored
    let peer_details = gs.connected_peers.get(&peer_id).unwrap();
    assert!(peer_details.extensions.is_some());

    // Simulate receiving duplicate extensions message from another peer
    let duplicate_rpc = RpcIn {
        messages: vec![],
        subscriptions: vec![],
        control_msgs: vec![ControlAction::Extensions(Some(Extensions {
            test_extension: Some(true),
            partial_messages: None,
        }))],
        test_extension: None,
        #[cfg(feature = "partial_messages")]
        partial_message: None,
    };

    gs.on_connection_handler_event(
        peer_id,
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: duplicate_rpc,
            invalid_messages: vec![],
        },
    );

    // Extensions should still be present (not cleared or changed)
    let peer_details = gs.connected_peers.get(&peer_id).unwrap();
    let test_extension = peer_details.extensions.unwrap().test_extension.unwrap();
    assert!(!test_extension);
}

/// A simple bitmap-based test implementation of the `Partial` trait.
/// Uses a fixed number of parts where each part is represented by a bit in a u8 bitmap.
#[derive(Clone, Debug)]
pub(crate) struct Bitmap {
    /// Unique identifier for the message group
    group_id: [u8; 8],
    /// Bitmap of which parts this message has (1 = has, 0 = missing)
    metadata: u8,
    /// The actual part data (indexed 0-7)
    parts: [[u8; 1024]; 8],
}

impl Bitmap {
    pub(crate) fn new(group_id: [u8; 8]) -> Self {
        Self {
            parts: [[0; 1024]; 8],
            metadata: 0,
            group_id,
        }
    }

    /// Fill the bitmap parts according to the provided metadata.
    pub(crate) fn fill_parts(&mut self, metadata: u8) {
        // Convert group_id to u64 using big-endian
        let start = u64::from_be_bytes(self.group_id);
        self.metadata |= metadata;

        for (i, p) in self.parts.iter_mut().enumerate() {
            // Part is not included in the metadata,
            // skip filling it.
            if (metadata & (1 << i)) == 0 {
                continue;
            }

            let mut counter = start + (i as u64) * (1024 / 8);
            let mut part = [0u8; 1024];

            for j in 0..(1024 / 8) {
                let bytes = counter.to_be_bytes();
                let offset = j * 8;
                part[offset..offset + 8].copy_from_slice(&bytes);
                counter += 1;
            }

            *p = part;
        }
    }

    /// Extend our partial data from a received message.
    pub(crate) fn extend_from_encoded_partial_message(
        &mut self,
        data: &[u8],
    ) -> Result<(), PartialError> {
        if data.len() < 1 + self.group_id.len() {
            return Err(PartialError::InvalidFormat);
        }

        let bitmap = data[0];
        let data = &data[1..];
        let (data, group_id) = data.split_at(data.len() - self.group_id.len());
        if group_id != self.group_id {
            return Err(PartialError::WrongGroup {
                received: group_id.to_vec(),
            });
        }

        if data.len() % 1024 != 0 {
            return Err(PartialError::InvalidFormat);
        }

        let mut offset = 0;
        for (i, field) in self.parts.iter_mut().enumerate() {
            if (bitmap >> i) & 1 == 0 {
                continue;
            }

            if (self.metadata >> i) & 1 == 1 {
                continue; // we already have this
            }

            if offset + 1024 > data.len() {
                return Err(PartialError::InvalidFormat);
            }

            self.metadata |= 1 << i;
            field.copy_from_slice(&data[offset..offset + 1024]);
            offset += 1024;
        }

        Ok(())
    }
}

impl Partial for Bitmap {
    fn group_id(&self) -> Vec<u8> {
        self.group_id.to_vec()
    }

    fn metadata(&self) -> Box<dyn Metadata> {
        Box::new(MetadataBitmap {
            bitmap: [self.metadata; 1],
        })
    }

    fn partial_action_from_metadata(
        &self,
        _peer_id: PeerId,
        metadata: Option<&[u8]>,
    ) -> Result<PartialAction, PartialError> {
        let metadata = metadata.unwrap_or(&[0u8]);

        if metadata.len() != 1 {
            return Err(PartialError::InvalidFormat);
        }

        let bitmap = metadata[0];
        let mut response_bitmap: u8 = 0;

        let part_count = bitmap.count_ones() as usize;
        let mut data = Vec::with_capacity(1 + 1024 * part_count + self.group_id.len());

        let mut peer_has_useful_data = false;
        data.push(0);

        for (i, field) in self.parts.iter().enumerate() {
            if (bitmap >> i) & 1 != 0 {
                if !peer_has_useful_data && (self.metadata >> i) & 1 == 0 {
                    // They have something we don't
                    peer_has_useful_data = true;
                }

                // They have this part
                continue;
            }
            if (self.metadata >> i) & 1 == 0 {
                continue; // Not available
            }

            response_bitmap |= 1 << i;

            data.extend_from_slice(field);
        }

        if response_bitmap == 0 {
            return Ok(PartialAction {
                need: peer_has_useful_data,
                send: None,
            });
        }

        // Set the correct bitmap in the first byte
        data[0] = response_bitmap;
        data.extend_from_slice(&self.group_id);
        let bitmap = MetadataBitmap {
            bitmap: [metadata[0] | response_bitmap],
        };

        Ok(PartialAction {
            need: peer_has_useful_data,
            send: Some((data, Box::new(bitmap))),
        })
    }
}

#[derive(Debug)]
struct MetadataBitmap {
    bitmap: [u8; 1],
}

impl Metadata for MetadataBitmap {
    fn as_slice(&self) -> &[u8] {
        self.bitmap.as_slice()
    }

    fn update(&mut self, data: &[u8]) -> Result<bool, PartialError> {
        if data.len() != 1 {
            return Err(PartialError::InvalidFormat);
        }

        let before = self.bitmap[0];
        self.bitmap[0] |= data[0];
        Ok(self.bitmap[0] != before)
    }
}

/// Verifies that:
/// - A peer (peer1) with a complete message publishes it to a peer with no parts.
/// - A second peer (peer2), starting with an empty message, requests all missing parts.
/// - Peer2 successfully reconstructs the full message from the parts received from peer1.
#[test]
fn test_full_data_provider_to_requester() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();

    let mut state1 = State::default();
    let mut state2 = State::default();
    // Both subscribe to the topic with partial support
    state1.subscribe(topic_hash.clone(), true, true);
    state2.subscribe(topic_hash.clone(), true, true);

    // Set up peer subscriptions (each knows about the other)
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );

    state2.peer_subscribed(
        &peer1,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );

    // Peer1 has the full message (all 8 parts = 0b11111111)
    let mut full_message = Bitmap::new(group_id);
    full_message.fill_parts(0b11111111);

    let mut empty_message = Bitmap::new(group_id);
    // Peer1 publishes its full message
    let actions1 = state1
        .handle_publish(topic_hash.clone(), full_message.clone(), vec![peer2])
        .expect("Publish should succeed");

    // Verify peer1 sent a partial message with data
    assert_eq!(actions1.len(), 1, "Should have one action");
    let PublishAction::SendMessage {
        peer_id: sent_peer,
        rpc: sent_rpc,
    } = &actions1[0]
    else {
        panic!("Expected SendMessage action");
    };

    assert_eq!(*sent_peer, peer2, "Should send to peer2");

    let RpcOut::PartialMessage(partial_msg) = sent_rpc else {
        panic!("Expected PartialMessage");
    };

    assert!(partial_msg.body.is_some(), "Should include body with data");
    assert!(
        partial_msg.metadata.is_some(),
        "Should include metadata (bitmap)"
    );

    // Peer2 publishes its empty message (requesting data)
    let actions2 = state2
        .handle_publish(topic_hash.clone(), empty_message.clone(), vec![peer1])
        .expect("Publish should succeed");
    // Peer2 should send a metadata-only message (requesting parts)
    assert_eq!(actions2.len(), 1, "Should have one action");
    let partial_msg2 = match &actions2[0] {
        PublishAction::SendMessage { rpc, .. } => match rpc {
            RpcOut::PartialMessage(pm) => pm,
            _ => panic!("Expected PartialMessage"),
        },
        _ => panic!("Expected SendMessage action"),
    };

    // Body should be None since peer2 has nothing to send
    assert!(
        partial_msg2.body.is_none(),
        "Peer2 should not send body (has no data)"
    );
    // Metadata should be 0 as peer2 doesn't have any part.
    assert_eq!(partial_msg2.metadata, Some(vec![0]));

    // Extract body and verify peer2 can reconstruct the full message
    // Simulate peer2 receiving the partial message from peer1
    let mut actions = state2.handle_received(peer1, partial_msg.clone());
    let ReceivedAction::EmitEvent {
        topic_hash: received_topic,
        peer_id: received_peer,
        group_id: received_group_id,
        message: Some(received_message),
        metadata: received_metadata,
    } = actions
        .pop()
        .expect("Peer2 handle_receive should have a single EmitEvent received action")
    else {
        panic!("Expected Emit Event since peer 2 doesn't have any part");
    };

    assert!(
        actions.is_empty(),
        "Peer2 handle_received action should only be EmitEvent"
    );

    assert_eq!(received_topic, topic_hash);
    assert_eq!(received_group_id, group_id);
    assert_eq!(received_peer, peer1);
    assert_eq!(received_metadata, Some(vec![0b11111111]));

    empty_message
        .extend_from_encoded_partial_message(&received_message)
        .expect("Should decode received data");
    assert_eq!(
        empty_message.metadata, 0b11111111,
        "Peer2 should have all 8 parts after receiving missing ones"
    );
}

/// Verifies that:
/// - Peer1 with all parts only sends the parts that peer2 is missing.
/// - Peer2 (with parts 0,2,4,6 = 0b01010101) receives only parts 1,3,5,7 (0b10101010).
/// - Peer2 successfully reconstructs the full message via handle_received.
#[test]
fn test_partial_overlap_exchange() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    let mut state2 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    state2.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    state2.peer_subscribed(
        &peer1,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Peer1 has all 8 parts
    let mut full_message = Bitmap::new(group_id);
    full_message.fill_parts(0b11111111);
    // Peer2 has only even parts (0,2,4,6)
    let mut peer2_message = Bitmap::new(group_id);
    peer2_message.fill_parts(0b01010101);
    // Peer1 publishes its full message
    let actions1 = state1
        .handle_publish(topic_hash.clone(), full_message.clone(), vec![peer2])
        .expect("Publish should succeed");
    assert_eq!(actions1.len(), 1);
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg1),
        ..
    } = &actions1[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    // Peer1 should send all parts (peer2's metadata unknown yet)
    assert!(partial_msg1.body.is_some());
    assert_eq!(partial_msg1.body.as_ref().unwrap()[0], 0b11111111);
    // Peer2 publishes its partial message
    let actions2 = state2
        .handle_publish(topic_hash.clone(), peer2_message.clone(), vec![peer1])
        .expect("Publish should succeed");
    assert_eq!(actions2.len(), 1);
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg2),
        ..
    } = &actions2[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    // Peer2 sends even parts
    assert!(partial_msg2.body.is_some());
    assert_eq!(partial_msg2.body.as_ref().unwrap()[0], 0b01010101);
    assert_eq!(partial_msg2.metadata, Some(vec![0b01010101]));
    // State1 receives peer2's partial (with even parts)
    // State1 has full message cached, should respond with missing odd parts
    let mut received_actions1 = state1.handle_received(peer2, partial_msg2.clone());
    // State1 should only publish a response (no EmitEvent since it already has all parts)
    assert_eq!(
        received_actions1.len(),
        1,
        "Should have only Publish action"
    );
    let ReceivedAction::Publish(PublishAction::SendMessage {
        peer_id: response_peer,
        rpc: RpcOut::PartialMessage(response_msg),
    }) = received_actions1.remove(0)
    else {
        panic!("Expected Publish with SendMessage");
    };
    assert_eq!(response_peer, peer2);
    let response_body = response_msg.body.as_ref().expect("Should have body");
    assert_eq!(
        response_body[0], 0b10101010,
        "Should only send odd parts (1,3,5,7) that peer2 is missing"
    );
    // State2 receives peer1's full message
    // State2 has even parts cached, should emit event with received data
    let mut received_actions2 = state2.handle_received(peer1, partial_msg1.clone());
    // State2 should only emit event (it has even parts, receives all parts,
    // but peer1 already has everything so no need to publish response)
    assert_eq!(
        received_actions2.len(),
        1,
        "Should have only EmitEvent action"
    );
    let ReceivedAction::EmitEvent {
        topic_hash: received_topic,
        peer_id: received_peer,
        group_id: received_group_id,
        message: received_message,
        metadata: received_metadata,
    } = received_actions2.remove(0)
    else {
        panic!("Expected EmitEvent");
    };
    assert_eq!(received_topic, topic_hash);
    assert_eq!(received_peer, peer1);
    assert_eq!(received_group_id, group_id);
    assert_eq!(received_metadata, Some(vec![0b11111111]));
    let received_body = received_message.expect("Should have body");
    assert_eq!(received_body[0], 0b11111111, "Should receive all parts");
    // Peer2 extends its message with received data
    peer2_message
        .extend_from_encoded_partial_message(&received_body)
        .expect("Should decode received data");
    assert_eq!(
        peer2_message.metadata, 0b11111111,
        "Peer2 should have all 8 parts after receiving from peer1"
    );
}

/// Verifies that:
/// - Peer1 has even parts (0,2,4,6 = 0b01010101).
/// - Peer2 has odd parts (1,3,5,7 = 0b10101010).
/// - Both peers exchange via handle_received and can reconstruct full message.
#[test]
fn test_partial_symmetric_half_exchange() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    let mut state2 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    state2.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    state2.peer_subscribed(
        &peer1,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Peer1 has even parts (0,2,4,6)
    let mut peer1_message = Bitmap::new(group_id);
    peer1_message.fill_parts(0b01010101);
    // Peer2 has odd parts (1,3,5,7)
    let mut peer2_message = Bitmap::new(group_id);
    peer2_message.fill_parts(0b10101010);
    // Both publish their partials
    let actions1 = state1
        .handle_publish(topic_hash.clone(), peer1_message.clone(), vec![peer2])
        .expect("Publish should succeed");
    let actions2 = state2
        .handle_publish(topic_hash.clone(), peer2_message.clone(), vec![peer1])
        .expect("Publish should succeed");
    assert_eq!(actions1.len(), 1);
    assert_eq!(actions2.len(), 1);
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg1),
        ..
    } = &actions1[0]
    else {
        panic!("Expected SendMessage with PartialMessage from peer1");
    };
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg2),
        ..
    } = &actions2[0]
    else {
        panic!("Expected SendMessage with PartialMessage from peer2");
    };
    // Verify each sends their respective parts
    assert_eq!(
        partial_msg1.body.as_ref().unwrap()[0],
        0b01010101,
        "Peer1 sends even parts"
    );
    assert_eq!(
        partial_msg2.body.as_ref().unwrap()[0],
        0b10101010,
        "Peer2 sends odd parts"
    );

    // State1 receives peer2's odd parts
    // State1 has even parts cached, should:
    // - Emit event with odd parts (data it needs)
    // - Publish response with even parts (data peer2 needs)
    let mut received_actions1 = state1.handle_received(peer2, partial_msg2.clone());
    assert_eq!(
        received_actions1.len(),
        2,
        "Should have EmitEvent and Publish"
    );
    // EmitEvent comes first
    let ReceivedAction::EmitEvent {
        topic_hash: received_topic1,
        peer_id: received_peer1,
        group_id: received_group_id1,
        message: received_message1,
        metadata: received_metadata1,
    } = received_actions1.remove(0)
    else {
        panic!("Expected EmitEvent as first action");
    };
    assert_eq!(received_topic1, topic_hash);
    assert_eq!(received_peer1, peer2);
    assert_eq!(received_group_id1, group_id);
    assert_eq!(received_metadata1, Some(vec![0b10101010]));
    let received_body1 = received_message1.expect("Should have body");
    assert_eq!(received_body1[0], 0b10101010, "Should receive odd parts");
    // Publish comes second
    let ReceivedAction::Publish(PublishAction::SendMessage {
        peer_id: response_peer1,
        rpc: RpcOut::PartialMessage(response_msg1),
    }) = received_actions1.remove(0)
    else {
        panic!("Expected Publish with SendMessage as second action");
    };
    assert_eq!(response_peer1, peer2);
    let response_body1 = response_msg1.body.as_ref().expect("Should have body");
    assert_eq!(
        response_body1[0], 0b01010101,
        "Should send even parts to peer2"
    );
    // Peer1 extends its message with received odd parts
    peer1_message
        .extend_from_encoded_partial_message(&received_body1)
        .expect("Should decode received data");
    assert_eq!(
        peer1_message.metadata, 0b11111111,
        "Peer1 should have all 8 parts after receiving odd parts"
    );
    // State2 receives peer1's even parts
    // State2 has odd parts cached, should:
    // - Emit event with even parts (data it needs)
    // - Publish response with odd parts (data peer1 needs)
    let mut received_actions2 = state2.handle_received(peer1, partial_msg1.clone());
    assert_eq!(
        received_actions2.len(),
        2,
        "Should have EmitEvent and Publish"
    );
    // EmitEvent comes first
    let ReceivedAction::EmitEvent {
        topic_hash: received_topic2,
        peer_id: received_peer2,
        group_id: received_group_id2,
        message: received_message2,
        metadata: received_metadata2,
    } = received_actions2.remove(0)
    else {
        panic!("Expected EmitEvent as first action");
    };
    assert_eq!(received_topic2, topic_hash);
    assert_eq!(received_peer2, peer1);
    assert_eq!(received_group_id2, group_id);
    assert_eq!(received_metadata2, Some(vec![0b01010101]));
    let received_body2 = received_message2.expect("Should have body");
    assert_eq!(received_body2[0], 0b01010101, "Should receive even parts");
    // Publish comes second
    let ReceivedAction::Publish(PublishAction::SendMessage {
        peer_id: response_peer2,
        rpc: RpcOut::PartialMessage(response_msg2),
    }) = received_actions2.remove(0)
    else {
        panic!("Expected Publish with SendMessage as second action");
    };
    assert_eq!(response_peer2, peer1);
    let response_body2 = response_msg2.body.as_ref().expect("Should have body");
    assert_eq!(
        response_body2[0], 0b10101010,
        "Should send odd parts to peer1"
    );
    // Peer2 extends its message with received even parts
    peer2_message
        .extend_from_encoded_partial_message(&received_body2)
        .expect("Should decode received data");
    assert_eq!(
        peer2_message.metadata, 0b11111111,
        "Peer2 should have all 8 parts after receiving even parts"
    );
}

/// Verifies that:
/// - Both peers have the same parts (0,2,4,6 = 0b01010101).
/// - No body should be sent in handle_received since neither has data the other needs.
#[test]
fn test_no_redundant_transfer() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    let mut state2 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    state2.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    state2.peer_subscribed(
        &peer1,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Both have the same even parts
    let mut peer1_message = Bitmap::new(group_id);
    peer1_message.fill_parts(0b01010101);
    let mut peer2_message = Bitmap::new(group_id);
    peer2_message.fill_parts(0b01010101);
    // Both publish their partials
    let actions1 = state1
        .handle_publish(topic_hash.clone(), peer1_message.clone(), vec![peer2])
        .expect("Publish should succeed");
    let actions2 = state2
        .handle_publish(topic_hash.clone(), peer2_message.clone(), vec![peer1])
        .expect("Publish should succeed");
    assert_eq!(actions1.len(), 1);
    assert_eq!(actions2.len(), 1);
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg1),
        ..
    } = &actions1[0]
    else {
        panic!("Expected SendMessage with PartialMessage from peer1");
    };
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg2),
        ..
    } = &actions2[0]
    else {
        panic!("Expected SendMessage with PartialMessage from peer2");
    };
    // Both send even parts
    assert_eq!(partial_msg1.body.as_ref().unwrap()[0], 0b01010101);
    assert_eq!(partial_msg2.body.as_ref().unwrap()[0], 0b01010101);
    // State1 receives peer2's message (same parts)
    // Should return empty - no new data to emit, nothing to send
    let received_actions1 = state1.handle_received(peer2, partial_msg2.clone());
    assert!(
        received_actions1.is_empty(),
        "Should have no actions - peer2 has same data as peer1"
    );
    // State2 receives peer1's message (same parts)
    // Should return empty - no new data to emit, nothing to send
    let received_actions2 = state2.handle_received(peer1, partial_msg1.clone());
    assert!(
        received_actions2.is_empty(),
        "Should have no actions - peer1 has same data as peer2"
    );
}
