use core::slice;
use std::collections::BTreeSet;

use hashlink::LinkedHashMap;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{ConnectionId, NetworkBehaviour, ToSwarm};

use crate::{
    behaviour::tests::{add_peer_with_addr_and_kind, flush_events, DefaultBehaviourTestBuilder},
    handler::HandlerEvent,
    partial_messages::{
        Metadata, Partial, PartialAction, PartialError, PartialMessage, PublishAction,
        ReceivedAction, State, DEFAULT_PARTIAL_TTL,
    },
    queue::Queue,
    rpc_proto::proto,
    types::{ControlAction, Extensions, PeerDetails, PeerKind, RpcIn, RpcOut, SubscriptionOpts},
    Behaviour, ConfigBuilder, Event, MessageAuthenticity, TopicHash, ValidationMode,
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

    fn update_from_data(&mut self, data: &[u8]) -> Result<(), PartialError> {
        self.bitmap[0] |= data[0];
        Ok(())
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
fn test_overlap_exchange() {
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
fn test_symmetric_half_exchange() {
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

/// Verifies that:
/// - After TTL expires, cached partials are cleaned up.
/// - This is verified by observing that handle_received returns EmitEvent
///   (as if seeing the group_id for the first time) instead of Publish.
#[test]
fn test_heartbeat_ttl_expiry() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    let mut message = Bitmap::new(group_id);
    message.fill_parts(0b11111111);
    // Publish a partial message (caches it locally)
    let _actions = state1
        .handle_publish(topic_hash.clone(), message, vec![peer2])
        .expect("Publish should succeed");
    // Receive a partial from peer2 - since we have a cached local partial,
    // this should return Publish (responding with our data)
    let peer2_partial = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic_hash.clone(),
        body: Some(vec![0]),
        metadata: Some(vec![0b00000000]), // peer2 has nothing
    };
    let received_actions = state1.handle_received(peer2, peer2_partial.clone());
    // Should have Publish action (we have cached data to send)
    assert!(
        received_actions
            .iter()
            .any(|a| matches!(a, ReceivedAction::Publish(_))),
        "Before TTL expiry: should return Publish since we have cached local partial"
    );
    // Create empty mesh/fanout for heartbeat (no gossip, just TTL decrement)
    let empty_mesh = std::collections::HashMap::new();
    let empty_fanout = std::collections::HashMap::new();
    // Run heartbeats until TTL expires (DEFAULT_PARTIAL_TTL = 5)
    for _ in 0..DEFAULT_PARTIAL_TTL {
        let _ = state1.heartbeat(&empty_mesh, &empty_fanout, 0, 0.0, 100);
    }
    // Now receive the same partial again - since local cache expired,
    // this should return EmitEvent (as if first time seeing this group_id)
    let received_actions_after = state1.handle_received(peer2, peer2_partial.clone());
    // Should have EmitEvent (no cached local partial anymore)
    assert_eq!(received_actions_after.len(), 1);
    assert!(
        received_actions_after
            .iter()
            .any(|a| matches!(a, ReceivedAction::EmitEvent { .. })),
        "After TTL expiry: should return EmitEvent since local partial cache expired"
    );
}

/// Verifies that:
/// - When a peer disconnects, all its partial message state is cleaned up.
/// - This follows the same behavior as full messages.
/// - After disconnect and reconnect, we re-send all data (no knowledge of what peer has).
#[test]
fn test_peer_disconnect_cleanup() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Publish a partial to establish state with peer2
    let mut message = Bitmap::new(group_id);
    message.fill_parts(0b11111111);
    let _ = state1
        .handle_publish(topic_hash.clone(), message.clone(), vec![peer2])
        .expect("Publish should succeed");
    // Receive a partial from peer2 to create remote partial state
    let peer2_partial = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic_hash.clone(),
        body: Some(vec![0]),
        metadata: Some(vec![0b01010101]), // peer2 has even parts
    };
    let actions = state1.handle_received(peer2, peer2_partial.clone());
    assert_eq!(actions.len(), 1);
    let ReceivedAction::Publish(PublishAction::SendMessage {
        peer_id,
        rpc: RpcOut::PartialMessage(partial_msg),
    }) = &actions[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    assert_eq!(*peer_id, peer2);
    let body = partial_msg.body.as_ref().expect("Should have body");
    assert_eq!(
        body[0], 0b10101010,
        "Should send all parts after reconnect (peer metadata was cleaned up)"
    );

    // Now disconnect peer2
    state1.peer_disconnected(peer2);
    // Re-subscribe peer2 (simulating reconnection)
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Publish again - if peer2's metadata was preserved, we'd only send
    // missing parts (0b10101010). Since it was cleaned up on disconnect,
    // we send all parts (0b11111111).
    let actions = state1
        .handle_publish(topic_hash.clone(), message, vec![peer2])
        .expect("Publish should succeed");
    assert_eq!(actions.len(), 1);
    let PublishAction::SendMessage {
        peer_id,
        rpc: RpcOut::PartialMessage(partial_msg),
    } = &actions[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    assert_eq!(*peer_id, peer2);
    // Should send all parts since peer2's metadata was wiped on disconnect
    let body = partial_msg.body.as_ref().expect("Should have body");
    assert_eq!(
        body[0], 0b11111111,
        "Should send all parts after reconnect (peer metadata was cleaned up)"
    );
}

/// Verifies that:
/// - When we unsubscribe from a topic, our local partial message state is cleaned up.
/// - After re-subscribing, publishing is treated as fresh (no cached local partial).
#[test]
fn test_unsubscribe_cleanup() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Publish a partial to cache it locally
    let mut message = Bitmap::new(group_id);
    message.fill_parts(0b11111111);
    let _actions = state1
        .handle_publish(topic_hash.clone(), message, vec![peer2])
        .expect("Publish should succeed");
    // Receive from peer2 - should return Publish (we have cached local partial)
    let peer2_partial = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic_hash.clone(),
        body: Some(vec![0]),
        metadata: Some(vec![0b00000000]), // peer2 has nothing
    };
    let received_actions = state1.handle_received(peer2, peer2_partial.clone());
    assert!(
        received_actions
            .iter()
            .any(|a| matches!(a, ReceivedAction::Publish(_))),
        "Before unsubscribe: should return Publish since we have cached local partial"
    );
    // Unsubscribe from the topic
    state1.unsubscribe(&topic_hash);
    // Re-subscribe to the topic
    state1.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Receive the same partial again from peer2
    // Since our local cache was cleaned up, should return EmitEvent (no local partial)
    let received_actions_after = state1.handle_received(peer2, peer2_partial.clone());
    assert_eq!(received_actions_after.len(), 1);
    assert!(
        received_actions_after
            .iter()
            .any(|a| matches!(a, ReceivedAction::EmitEvent { .. })),
        "After unsubscribe: should return EmitEvent since local partial cache was cleaned up"
    );
}

/// Verifies that:
/// - When a peer unsubscribes from a topic, their partial message state for that topic is cleaned up.
/// - After the peer re-subscribes, we treat them as fresh (no knowledge of what they have).
#[test]
fn test_peer_unsubscribed_cleanup() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Publish a partial to establish state with peer2
    let mut message = Bitmap::new(group_id);
    message.fill_parts(0b11111111);
    let _actions = state1
        .handle_publish(topic_hash.clone(), message, vec![peer2])
        .expect("Publish should succeed");
    // Receive a partial from peer2 to create remote partial state
    let peer2_partial = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic_hash.clone(),
        body: Some(vec![0]),
        metadata: Some(vec![0b01010101]), // peer2 has even parts
    };
    let _ = state1.handle_received(peer2, peer2_partial.clone());
    // Peer2 unsubscribes from the topic
    state1.peer_unsubscribed(peer2, &topic_hash);
    // Peer2 re-subscribes to the topic
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Publish again - if peer2's metadata was preserved, we'd only send
    // missing parts (0b10101010). Since it was cleaned up on unsubscribe,
    // we send all parts (0b11111111).
    let mut new_message = Bitmap::new(group_id);
    new_message.fill_parts(0b11111111);
    let actions = state1
        .handle_publish(topic_hash.clone(), new_message, vec![peer2])
        .expect("Publish should succeed");
    assert_eq!(actions.len(), 1);
    let PublishAction::SendMessage {
        peer_id,
        rpc: RpcOut::PartialMessage(partial_msg),
    } = &actions[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    assert_eq!(*peer_id, peer2);
    // Should send all parts since peer2's metadata was wiped on unsubscribe
    let body = partial_msg.body.as_ref().expect("Should have body");
    assert_eq!(
        body[0], 0b11111111,
        "Should send all parts after peer re-subscribes (peer metadata was cleaned up)"
    );
}

/// Verifies that:
/// - When a peer unsubscribes from one topic, their state for other topics is preserved.
/// - We still know what parts they have for topics they remain subscribed to.
#[test]
fn test_peer_unsubscribed_preserves_other_topics() {
    let topic1 = TopicHash::from_raw("topic-1");
    let topic2 = TopicHash::from_raw("topic-2");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer2 = PeerId::random();
    let mut state1 = State::default();
    // Subscribe to both topics
    state1.subscribe(topic1.clone(), true, true);
    state1.subscribe(topic2.clone(), true, true);
    // Peer2 subscribes to both topics
    state1.peer_subscribed(
        &peer2,
        topic1.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    state1.peer_subscribed(
        &peer2,
        topic2.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Publish partials on both topics
    let mut message = Bitmap::new(group_id);
    message.fill_parts(0b11111111);
    let _actions1 = state1
        .handle_publish(topic1.clone(), message.clone(), vec![peer2])
        .expect("Publish should succeed");
    // Receive partials from peer2 on both topics (peer2 has even parts on both)
    let peer2_partial_topic1 = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic1.clone(),
        body: Some(vec![0]),
        metadata: Some(vec![0b01010101]),
    };
    let _ = state1.handle_received(peer2, peer2_partial_topic1);
    let peer2_partial_topic2 = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic2.clone(),
        body: Some(vec![0]),
        metadata: Some(vec![0b01010101]),
    };
    let _ = state1.handle_received(peer2, peer2_partial_topic2);
    // Peer2 unsubscribes from topic1 only
    state1.peer_unsubscribed(peer2, &topic1);
    // Publish on topic1 - peer2's state was cleared, should send all parts
    // Re-subscribe peer2 to topic1 first
    state1.peer_subscribed(
        &peer2,
        topic1.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    let actions1 = state1
        .handle_publish(topic1.clone(), message.clone(), vec![peer2])
        .expect("Publish should succeed");
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg1),
        ..
    } = &actions1[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    // Topic1: Should send all parts (state was cleared)
    let body1 = partial_msg1.body.as_ref().expect("Should have body");
    assert_eq!(
        body1[0], 0b11111111,
        "Topic1: Should send all parts (peer metadata was cleared on unsubscribe)"
    );
    // Publish on topic2 - peer2's state was preserved, should send only missing parts
    let actions2 = state1
        .handle_publish(topic2.clone(), message, vec![peer2])
        .expect("Publish should succeed");
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg2),
        ..
    } = &actions2[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    // Topic2: Should send only odd parts (peer2's state preserved, they have even parts)
    let body2 = partial_msg2.body.as_ref().expect("Should have body");
    assert_eq!(
        body2[0], 0b10101010,
        "Topic2: Should send only missing parts (peer metadata preserved)"
    );
}

/// Verifies that:
/// - `requests_partial()` and `supports_partial()` return correct values based on peer subscription options.
/// - Options are correctly tracked per peer per topic.
#[test]
fn test_subscription_options_tracking() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let peer1 = PeerId::random();
    let peer2 = PeerId::random();
    let peer3 = PeerId::random();
    let mut state1 = State::default();
    state1.subscribe(topic_hash.clone(), true, true);
    // Peer1: requests and supports partial
    state1.peer_subscribed(
        &peer1,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    // Peer2: only supports partial (doesn't request)
    state1.peer_subscribed(
        &peer2,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: false,
            supports_partial: true,
        },
    );
    // Peer3: only requests partial (doesn't support)
    state1.peer_subscribed(
        &peer3,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: false,
        },
    );
    // Verify peer1
    assert!(
        state1.requests_partial(&peer1, &topic_hash),
        "Peer1 should request partial"
    );
    assert!(
        state1.supports_partial(&peer1, &topic_hash),
        "Peer1 should support partial"
    );
    // Verify peer2
    assert!(
        !state1.requests_partial(&peer2, &topic_hash),
        "Peer2 should not request partial"
    );
    assert!(
        state1.supports_partial(&peer2, &topic_hash),
        "Peer2 should support partial"
    );
    // Verify peer3
    assert!(
        state1.requests_partial(&peer3, &topic_hash),
        "Peer3 should request partial"
    );
    assert!(
        !state1.supports_partial(&peer3, &topic_hash),
        "Peer3 should not support partial"
    );
    // Verify unknown peer returns false
    let unknown_peer = PeerId::random();
    assert!(
        !state1.requests_partial(&unknown_peer, &topic_hash),
        "Unknown peer should not request partial"
    );
    assert!(
        !state1.supports_partial(&unknown_peer, &topic_hash),
        "Unknown peer should not support partial"
    );
    // Verify unknown topic returns false
    let unknown_topic = TopicHash::from_raw("unknown-topic");
    assert!(
        !state1.requests_partial(&peer1, &unknown_topic),
        "Known peer on unknown topic should not request partial"
    );
    assert!(
        !state1.supports_partial(&peer1, &unknown_topic),
        "Known peer on unknown topic should not support partial"
    );
}

/// Verifies that:
/// - When Node A receives data from Node B, `RemotePartial::metadata` is updated
///   via `update_from_data` to reflect that B knows A now has those parts.
/// - On the next `handle_publish`, no message is sent because B's view of A's
///   metadata (`RemotePartial::metadata`) already matches A's current state.
#[test]
fn test_handle_publish_skips_redundant_update_after_receiving_data() {
    let topic_hash = TopicHash::from_raw("test-topic");
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let peer_a = PeerId::random();
    let peer_b = PeerId::random();
    let mut state_a = State::default();
    state_a.subscribe(topic_hash.clone(), true, true);
    state_a.peer_subscribed(
        &peer_b,
        topic_hash.clone(),
        SubscriptionOpts {
            requests_partial: true,
            supports_partial: true,
        },
    );
    let mut node_a_message = Bitmap::new(group_id);
    node_a_message.fill_parts(0b00000001);
    // Step 1: Node A publishes to Node B
    // This initializes RemotePartial::metadata to 0b00000001
    let actions = state_a
        .handle_publish(topic_hash.clone(), node_a_message.clone(), vec![peer_b])
        .expect("Publish should succeed");
    assert_eq!(actions.len(), 1, "Should send one message");
    let PublishAction::SendMessage {
        rpc: RpcOut::PartialMessage(partial_msg),
        ..
    } = &actions[0]
    else {
        panic!("Expected SendMessage with PartialMessage");
    };
    assert_eq!(
        partial_msg.metadata,
        Some(vec![0b00000001]),
        "Should send metadata with part 0"
    );
    assert!(partial_msg.body.is_some(), "Should send body with part 0");
    assert_eq!(
        partial_msg.body.as_ref().unwrap()[0],
        0b00000001,
        "Body should contain part 0"
    );
    // Step 2: Node A receives parts 1-7 from Node B
    // The body bitmap is 0b11111110 (parts 1-7)
    // This should trigger update_from_data, updating RemotePartial::metadata
    // to 0b00000001 | 0b11111110 = 0b11111111
    let mut node_b_message = Bitmap::new(group_id);
    node_b_message.fill_parts(0b11111110); // B sends parts 1-7
    let action = node_b_message
        .partial_action_from_metadata(peer_a, Some(&[0b00000001])) // B knows A has part 0
        .expect("Should generate action");
    let (body, _) = action.send.expect("Should have data to send");
    let received_partial = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic_hash.clone(),
        body: Some(body),
        metadata: Some(vec![0b11111111]), // B has all parts
    };
    let received_actions = state_a.handle_received(peer_b, received_partial);
    // Should emit event with the received data and possibly send a response
    // (though A has nothing new to send to B since B has everything)
    assert!(
        received_actions
            .iter()
            .any(|a| matches!(a, ReceivedAction::EmitEvent { .. })),
        "Should emit event with received parts"
    );
    // Step 3: Node A updates its local message to have all parts
    node_a_message.fill_parts(0b11111111);
    // Step 4: Node A publishes again
    // RemotePartial::metadata should now be 0b11111111 (updated via update_from_data)
    // A's current metadata is also 0b11111111
    // metadata.update(0b11111111) should return false (no change)
    // Therefore, NO SendMessage action should be emitted
    let actions = state_a
        .handle_publish(topic_hash.clone(), node_a_message, vec![peer_b])
        .expect("Publish should succeed");
    assert!(
        actions.is_empty(),
        "Should NOT send any message - B already knows A has all parts (via update_from_data)"
    );
}

/// Verifies that:
/// - Two nodes exchange partial messages via the Behaviour API.
/// - Node1 publishes even parts (0b01010101) and receives odd parts from Node2.
/// - Event::Partial is emitted when receiving partial messages.
#[test]
fn test_partial_messages_two_node_exchange() {
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let (mut gs1, peers, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test-partial".into()])
        .to_subscribe(true)
        .peer_kind(PeerKind::Gossipsubv1_3)
        .requests_partial(true)
        .supports_partial(true)
        .create_network();
    let peer2 = peers[0];
    let topic_hash = topics[0].clone();
    // Node1 has even parts
    let mut node1_message = Bitmap::new(group_id);
    node1_message.fill_parts(0b01010101);
    // Node1 publishes its partial
    gs1.publish_partial(topic_hash.clone(), node1_message.clone())
        .expect("Publish should succeed");
    // Check that node1 sent a partial message to peer2
    // Note: Queue may also contain Subscribe messages from setup, so we search for PartialMessage
    let mut receiver_queue = queues.remove(&peer2).unwrap();
    let mut partial_msg1 = None;
    while !receiver_queue.is_empty() {
        if let Some(RpcOut::PartialMessage(pm)) = receiver_queue.try_pop() {
            partial_msg1 = Some(pm);
            break;
        }
    }
    let partial_msg1 = partial_msg1.expect("Should have sent PartialMessage");
    assert!(partial_msg1.body.is_some());
    assert_eq!(partial_msg1.body.as_ref().unwrap()[0], 0b01010101);
    gs1.events.clear();
    // Node2 has odd parts
    let mut node2_message = Bitmap::new(group_id);
    node2_message.fill_parts(0b10101010);
    // Get the encoded body that node2 would send to a peer with no parts
    let action = node2_message
        .partial_action_from_metadata(peer2, Some(&[0x00]))
        .unwrap();
    let (body, _) = action.send.expect("Should have data to send");
    // Create the partial message that node2 would send
    let node2_partial = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic_hash.clone(),
        body: Some(body),
        metadata: Some(vec![0b10101010]),
    };
    gs1.on_connection_handler_event(
        peer2,
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![],
                subscriptions: vec![],
                control_msgs: vec![],
                test_extension: None,
                partial_message: Some(node2_partial),
            },
            invalid_messages: vec![],
        },
    );
    let partial_event = gs1
        .events
        .iter()
        .find(|e| matches!(e, ToSwarm::GenerateEvent(Event::Partial { .. })));
    assert!(partial_event.is_some(), "Should emit Event::Partial");
    // Extract and verify the event
    let ToSwarm::GenerateEvent(Event::Partial {
        topic_hash: recv_topic,
        peer_id: recv_peer,
        group_id: recv_group,
        message: recv_body,
        metadata: recv_metadata,
    }) = partial_event.unwrap()
    else {
        panic!("Expected Event::Partial");
    };
    assert_eq!(*recv_topic, topic_hash);
    assert_eq!(*recv_peer, peer2);
    assert_eq!(*recv_group, group_id.to_vec());
    assert_eq!(*recv_metadata, Some(vec![0b10101010]));
    assert!(recv_body.is_some());
    assert_eq!(recv_body.as_ref().unwrap()[0], 0b10101010);
}

/// Verifies that:
/// - Partial messages are only sent to peers with `supports_partial: true`.
/// - Peers with `supports_partial: false` are filtered out from `publish_partial`.
#[test]
fn test_partial_subscription_options_respected() {
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    let (mut gs, _, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(0)
        .topics(vec!["test-partial".into()])
        .to_subscribe(true)
        .requests_partial(true)
        .supports_partial(true)
        .create_network();

    let topic_hash = topics[0].clone();

    // 2. Add peer1 with supports_partial: true
    let (_peer1, queue1) = add_peer_with_addr_and_kind(
        &mut gs,
        slice::from_ref(&topic_hash),
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Gossipsubv1_3),
        true, // requests_partial
        true, // supports_partial
    );

    // 3. Add peer2 with supports_partial: false
    let (_peer2, queue2) = add_peer_with_addr_and_kind(
        &mut gs,
        slice::from_ref(&topic_hash),
        false, // outbound
        false, // explicit
        Multiaddr::empty(),
        Some(PeerKind::Gossipsubv1_3),
        false, // requests_partial
        false, // supports_partial
    );

    // 4. Publish a partial message
    let mut message = Bitmap::new(group_id);
    message.fill_parts(0b11111111);
    gs.publish_partial(topic_hash, message)
        .expect("Publish should succeed");

    // 5. Verify peer1 received a PartialMessage
    let mut receiver_queue1 = queue1;
    let mut peer1_received_partial = false;
    while !receiver_queue1.is_empty() {
        if let Some(RpcOut::PartialMessage(_)) = receiver_queue1.try_pop() {
            peer1_received_partial = true;
            break;
        }
    }
    assert!(
        peer1_received_partial,
        "Peer with supports_partial=true should receive PartialMessage"
    );

    // 6. Verify peer2 did NOT receive a PartialMessage
    let mut receiver_queue2 = queue2;
    let mut peer2_received_partial = false;
    while !receiver_queue2.is_empty() {
        if let Some(RpcOut::PartialMessage(_)) = receiver_queue2.try_pop() {
            peer2_received_partial = true;
            break;
        }
    }
    assert!(
        !peer2_received_partial,
        "Peer with supports_partial=false should NOT receive PartialMessage"
    );
}

/// Verifies that:
/// - A peer with `supports_partial: true` but `requests_partial: false` receives a PartialMessage.
/// - The PartialMessage contains metadata but NO body.
/// - A peer with both `supports_partial: true` and `requests_partial: true` receives body AND metadata.
#[test]
fn test_partial_requests_partial_metadata_only() {
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let (mut gs, _, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(0)
        .topics(vec!["test-partial".into()])
        .to_subscribe(true)
        .requests_partial(true)
        .supports_partial(true)
        .create_network();
    let topic_hash = topics[0].clone();
    // Add peer1 with requests_partial: true, supports_partial: true
    let (_peer1, queue1) = add_peer_with_addr_and_kind(
        &mut gs,
        slice::from_ref(&topic_hash),
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Gossipsubv1_3),
        true, // requests_partial
        true, // supports_partial
    );
    // Add peer2 with requests_partial: false, supports_partial: true
    let (_peer2, queue2) = add_peer_with_addr_and_kind(
        &mut gs,
        slice::from_ref(&topic_hash),
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Gossipsubv1_3),
        false, // requests_partial
        true,  // supports_partial
    );
    // Publish a partial message with full data
    let mut message = Bitmap::new(group_id);
    message.fill_parts(0b11111111);
    gs.publish_partial(topic_hash.clone(), message)
        .expect("Publish should succeed");
    // Verify peer1 received a PartialMessage WITH body
    let mut receiver_queue1 = queue1;
    let mut peer1_partial = None;
    while !receiver_queue1.is_empty() {
        if let Some(RpcOut::PartialMessage(pm)) = receiver_queue1.try_pop() {
            peer1_partial = Some(pm);
            break;
        }
    }
    let peer1_partial = peer1_partial.expect("Peer1 should receive PartialMessage");
    assert!(
        peer1_partial.body.is_some(),
        "Peer with requests_partial=true should receive body"
    );
    assert!(
        peer1_partial.metadata.is_some(),
        "Peer with requests_partial=true should receive metadata"
    );
    // Verify peer2 received a PartialMessage WITHOUT body (metadata only)
    let mut receiver_queue2 = queue2;
    let mut peer2_partial = None;
    while !receiver_queue2.is_empty() {
        if let Some(RpcOut::PartialMessage(pm)) = receiver_queue2.try_pop() {
            peer2_partial = Some(pm);
            break;
        }
    }
    let peer2_partial = peer2_partial.expect("Peer2 should receive PartialMessage");
    assert!(
        peer2_partial.body.is_none(),
        "Peer with requests_partial=false should NOT receive body"
    );
    assert!(
        peer2_partial.metadata.is_some(),
        "Peer with requests_partial=false should still receive metadata"
    );
}

/// Verifies that:
/// - When receiving a partial message with complementary data, we respond with our parts.
/// - Both Event::Partial is emitted AND a response PartialMessage is sent.
/// - The response contains only the parts the peer is missing.
#[test]
fn test_partial_messages_response_on_receive() {
    let group_id: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let (mut gs, peers, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test-partial".into()])
        .to_subscribe(true)
        .peer_kind(PeerKind::Gossipsubv1_3)
        .requests_partial(true)
        .supports_partial(true)
        .create_network();
    let peer = peers[0];
    let topic_hash = topics[0].clone();
    // Local node publishes even parts (0b01010101) - caches them locally
    let mut local_message = Bitmap::new(group_id);
    local_message.fill_parts(0b01010101);
    gs.publish_partial(topic_hash.clone(), local_message.clone())
        .expect("Publish should succeed");
    // Drain the queue to clear the initial publish message
    let mut receiver_queue = queues.remove(&peer).unwrap();
    while !receiver_queue.is_empty() {
        let _ = receiver_queue.try_pop();
    }
    queues.insert(peer, receiver_queue);
    gs.events.clear();
    // Create the partial message from peer with odd parts (0b10101010)
    let mut peer_message = Bitmap::new(group_id);
    peer_message.fill_parts(0b10101010);
    let action = peer_message
        .partial_action_from_metadata(peer, Some(&[0b01010101])) // peer knows we have even parts
        .unwrap();
    let (body, _) = action.send.expect("Should have data to send");
    let peer_partial = PartialMessage {
        group_id: group_id.to_vec(),
        topic_hash: topic_hash.clone(),
        body: Some(body),
        metadata: Some(vec![0b10101010]),
    };
    // Deliver via on_connection_handler_event
    gs.on_connection_handler_event(
        peer,
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![],
                subscriptions: vec![],
                control_msgs: vec![],
                test_extension: None,
                partial_message: Some(peer_partial),
            },
            invalid_messages: vec![],
        },
    );
    // Verify Event::Partial was emitted with received odd parts
    let partial_event = gs
        .events
        .iter()
        .find(|e| matches!(e, ToSwarm::GenerateEvent(Event::Partial { .. })));
    assert!(partial_event.is_some(), "Should emit Event::Partial");
    let ToSwarm::GenerateEvent(Event::Partial {
        message: recv_body,
        metadata: recv_metadata,
        ..
    }) = partial_event.unwrap()
    else {
        panic!("Expected Event::Partial");
    };
    assert!(recv_body.is_some(), "Should have received body");
    assert_eq!(
        recv_body.as_ref().unwrap()[0],
        0b10101010,
        "Should receive odd parts from peer"
    );
    assert_eq!(*recv_metadata, Some(vec![0b10101010]));
    // Verify a response PartialMessage was sent back with even parts
    let mut receiver_queue = queues.remove(&peer).unwrap();
    let mut response_partial = None;
    while !receiver_queue.is_empty() {
        if let Some(RpcOut::PartialMessage(pm)) = receiver_queue.try_pop() {
            response_partial = Some(pm);
            break;
        }
    }
    let response_partial = response_partial.expect("Should send response PartialMessage");
    assert!(
        response_partial.body.is_some(),
        "Response should contain body with our parts"
    );
    assert_eq!(
        response_partial.body.as_ref().unwrap()[0],
        0b01010101,
        "Response should contain only even parts (what peer is missing)"
    );
    assert!(
        response_partial.metadata.is_some(),
        "Response should contain metadata"
    );
}
