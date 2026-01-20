// Copyright 2025 Sigma Prime Pty Ltd.
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

//! Tests for topic-specific configuration.

use std::collections::HashMap;

use asynchronous_codec::{Decoder, Encoder};
use bytes::BytesMut;
use libp2p_swarm::{ConnectionId, NetworkBehaviour, ToSwarm};

use super::DefaultBehaviourTestBuilder;
use crate::{
    config::{Config, ConfigBuilder, TopicMeshConfig},
    error::ValidationError,
    handler::HandlerEvent,
    protocol::GossipsubCodec,
    rpc_proto::proto,
    topic::TopicHash,
    types::{RawMessage, RpcIn, RpcOut},
    Event, IdentTopic as Topic, PublishError, ValidationMode,
};

/// Test that specific topic configurations are correctly applied
#[test]
fn test_topic_specific_config() {
    let topic_hash1 = Topic::new("topic1").hash();
    let topic_hash2 = Topic::new("topic2").hash();

    let topic_config1 = TopicMeshConfig {
        mesh_n: 5,
        mesh_n_low: 3,
        mesh_n_high: 10,
        mesh_outbound_min: 2,
    };

    let topic_config2 = TopicMeshConfig {
        mesh_n: 8,
        mesh_n_low: 4,
        mesh_n_high: 12,
        mesh_outbound_min: 3,
    };

    let config = ConfigBuilder::default()
        .set_topic_config(topic_hash1.clone(), topic_config1)
        .set_topic_config(topic_hash2.clone(), topic_config2)
        .build()
        .unwrap();

    assert_eq!(config.mesh_n_for_topic(&topic_hash1), 5);
    assert_eq!(config.mesh_n_low_for_topic(&topic_hash1), 3);
    assert_eq!(config.mesh_n_high_for_topic(&topic_hash1), 10);
    assert_eq!(config.mesh_outbound_min_for_topic(&topic_hash1), 2);

    assert_eq!(config.mesh_n_for_topic(&topic_hash2), 8);
    assert_eq!(config.mesh_n_low_for_topic(&topic_hash2), 4);
    assert_eq!(config.mesh_n_high_for_topic(&topic_hash2), 12);
    assert_eq!(config.mesh_outbound_min_for_topic(&topic_hash2), 3);

    let topic_hash3 = TopicHash::from_raw("topic3");

    assert_eq!(config.mesh_n_for_topic(&topic_hash3), config.mesh_n());
    assert_eq!(
        config.mesh_n_low_for_topic(&topic_hash3),
        config.mesh_n_low()
    );
    assert_eq!(
        config.mesh_n_high_for_topic(&topic_hash3),
        config.mesh_n_high()
    );
    assert_eq!(
        config.mesh_outbound_min_for_topic(&topic_hash3),
        config.mesh_outbound_min()
    );
}

/// Test mesh maintenance with topic-specific configurations
#[test]
fn test_topic_mesh_maintenance_with_specific_config() {
    let topic1_hash = TopicHash::from_raw("topic1");
    let topic2_hash = TopicHash::from_raw("topic2");

    let topic_config1 = TopicMeshConfig {
        mesh_n: 4,
        mesh_n_low: 2,
        mesh_n_high: 6,
        mesh_outbound_min: 1,
    };

    let topic_config2 = TopicMeshConfig {
        mesh_n: 8,
        mesh_n_low: 4,
        mesh_n_high: 12,
        mesh_outbound_min: 3,
    };

    let config = ConfigBuilder::default()
        .set_topic_config(topic1_hash, topic_config1)
        .set_topic_config(topic2_hash, topic_config2)
        .build()
        .unwrap();

    let (mut gs, _, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(15)
        .topics(vec!["topic1".into(), "topic2".into()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    assert_eq!(
        gs.mesh.get(&topic_hashes[0]).unwrap().len(),
        2,
        "topic1 should have mesh_n 2 peers"
    );
    assert_eq!(
        gs.mesh.get(&topic_hashes[1]).unwrap().len(),
        4,
        "topic2 should have mesh_n 4 peers"
    );

    // run a heartbeat
    gs.heartbeat();

    assert_eq!(
        gs.mesh.get(&topic_hashes[0]).unwrap().len(),
        2,
        "topic1 should maintain mesh_n 2 peers after heartbeat"
    );
    assert_eq!(
        gs.mesh.get(&topic_hashes[1]).unwrap().len(),
        4,
        "topic2 should maintain mesh_n 4 peers after heartbeat"
    );
}

/// Test mesh addition with topic-specific configuration
#[test]
fn test_mesh_addition_with_topic_config() {
    let topic = String::from("topic1");
    let topic_hash = TopicHash::from_raw(topic.clone());

    let topic_config = TopicMeshConfig {
        mesh_n: 6,
        mesh_n_low: 3,
        mesh_n_high: 9,
        mesh_outbound_min: 2,
    };

    let config = ConfigBuilder::default()
        .set_topic_config(topic_hash.clone(), topic_config.clone())
        .build()
        .unwrap();

    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_for_topic(&topic_hash) + 1)
        .topics(vec![topic])
        .to_subscribe(true)
        .gs_config(config.clone())
        .create_network();

    let to_remove_peers = 1;

    for peer in peers.iter().take(to_remove_peers) {
        gs.handle_prune(
            peer,
            topics.iter().map(|h| (h.clone(), vec![], None)).collect(),
        );
    }

    assert_eq!(
        gs.mesh.get(&topics[0]).unwrap().len(),
        config.mesh_n_low_for_topic(&topic_hash) - 1
    );

    // run a heartbeat
    gs.heartbeat();

    // Peers should be added to reach mesh_n
    assert_eq!(
        gs.mesh.get(&topics[0]).unwrap().len(),
        config.mesh_n_for_topic(&topic_hash)
    );
}

/// Test mesh subtraction with topic-specific configuration
#[test]
fn test_mesh_subtraction_with_topic_config() {
    let topic = String::from("topic1");
    let topic_hash = TopicHash::from_raw(topic.clone());

    let mesh_n = 5;
    let mesh_n_high = 7;

    let topic_config = TopicMeshConfig {
        mesh_n,
        mesh_n_high,
        mesh_n_low: 3,
        mesh_outbound_min: 2,
    };

    let config = ConfigBuilder::default()
        .set_topic_config(topic_hash.clone(), topic_config)
        .build()
        .unwrap();

    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(mesh_n_high)
        .topics(vec![topic])
        .to_subscribe(true)
        .gs_config(config.clone())
        .outbound(mesh_n_high)
        .create_network();

    // graft all peers
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    assert_eq!(
        gs.mesh.get(&topics[0]).unwrap().len(),
        mesh_n_high,
        "Initially all peers should be in the mesh"
    );

    // run a heartbeat
    gs.heartbeat();

    // Peers should be removed to reach mesh_n
    assert_eq!(
        gs.mesh.get(&topics[0]).unwrap().len(),
        5,
        "After heartbeat, mesh should be reduced to mesh_n 5 peers"
    );
}

/// Tests that if a mesh reaches `mesh_n_high`,
/// but is only composed of outbound peers, it is not reduced to `mesh_n`.
#[test]
fn test_mesh_subtraction_with_topic_config_min_outbound() {
    let topic = String::from("topic1");
    let topic_hash = TopicHash::from_raw(topic.clone());

    let mesh_n = 5;
    let mesh_n_high = 7;

    let topic_config = TopicMeshConfig {
        mesh_n,
        mesh_n_high,
        mesh_n_low: 3,
        mesh_outbound_min: 7,
    };

    let config = ConfigBuilder::default()
        .set_topic_config(topic_hash.clone(), topic_config)
        .build()
        .unwrap();

    let peer_no = 12;

    // make all outbound connections.
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(peer_no)
        .topics(vec![topic])
        .to_subscribe(true)
        .gs_config(config.clone())
        .outbound(peer_no)
        .create_network();

    // graft all peers
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    assert_eq!(
        gs.mesh.get(&topics[0]).unwrap().len(),
        mesh_n_high,
        "Initially mesh should be {mesh_n_high}"
    );

    // run a heartbeat
    gs.heartbeat();

    assert_eq!(
        gs.mesh.get(&topics[0]).unwrap().len(),
        mesh_n_high,
        "After heartbeat, mesh should still be {mesh_n_high} as these are all outbound peers"
    );
}

/// Test behavior with multiple topics having different configs
#[test]
fn test_multiple_topics_with_different_configs() {
    let topic1 = String::from("topic1");
    let topic2 = String::from("topic2");
    let topic3 = String::from("topic3");

    let topic_hash1 = TopicHash::from_raw(topic1.clone());
    let topic_hash2 = TopicHash::from_raw(topic2.clone());
    let topic_hash3 = TopicHash::from_raw(topic3.clone());

    let config1 = TopicMeshConfig {
        mesh_n: 4,
        mesh_n_low: 3,
        mesh_n_high: 6,
        mesh_outbound_min: 1,
    };

    let config2 = TopicMeshConfig {
        mesh_n: 6,
        mesh_n_low: 4,
        mesh_n_high: 9,
        mesh_outbound_min: 2,
    };

    let config3 = TopicMeshConfig {
        mesh_n: 9,
        mesh_n_low: 6,
        mesh_n_high: 13,
        mesh_outbound_min: 3,
    };

    let config = ConfigBuilder::default()
        .set_topic_config(topic_hash1.clone(), config1)
        .set_topic_config(topic_hash2.clone(), config2)
        .set_topic_config(topic_hash3.clone(), config3)
        .build()
        .unwrap();

    // Create network with many peers and three topics
    let (mut gs, _, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(35)
        .topics(vec![topic1, topic2, topic3])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    // Check that mesh sizes match each topic's config
    assert_eq!(
        gs.mesh.get(&topic_hashes[0]).unwrap().len(),
        3,
        "topic1 should have 3 peers"
    );
    assert_eq!(
        gs.mesh.get(&topic_hashes[1]).unwrap().len(),
        4,
        "topic2 should have 4 peers"
    );
    assert_eq!(
        gs.mesh.get(&topic_hashes[2]).unwrap().len(),
        6,
        "topic3 should have 6 peers"
    );

    // run a heartbeat
    gs.heartbeat();

    // Verify mesh sizes remain correct after maintenance. The mesh parameters are > mesh_n_low and
    // < mesh_n_high so the implementation will maintain the mesh at mesh_n_low.
    assert_eq!(
        gs.mesh.get(&topic_hashes[0]).unwrap().len(),
        3,
        "topic1 should maintain 3 peers after heartbeat"
    );
    assert_eq!(
        gs.mesh.get(&topic_hashes[1]).unwrap().len(),
        4,
        "topic2 should maintain 4 peers after heartbeat"
    );
    assert_eq!(
        gs.mesh.get(&topic_hashes[2]).unwrap().len(),
        6,
        "topic3 should maintain 6 peers after heartbeat"
    );

    // Unsubscribe from topic1
    assert!(
        gs.unsubscribe(&Topic::new(topic_hashes[0].to_string())),
        "Should unsubscribe successfully"
    );

    // verify it's removed from mesh
    assert!(
        !gs.mesh.contains_key(&topic_hashes[0]),
        "topic1 should be removed from mesh after unsubscribe"
    );

    // re-subscribe to topic1
    assert!(
        gs.subscribe(&Topic::new(topic_hashes[0].to_string()))
            .unwrap(),
        "Should subscribe successfully"
    );

    // Verify mesh is recreated with correct size
    assert_eq!(
        gs.mesh.get(&topic_hashes[0]).unwrap().len(),
        4,
        "topic1 should have mesh_n 4 peers after re-subscribe"
    );
}

/// Test fanout behavior with topic-specific configuration
#[test]
fn test_fanout_with_topic_config() {
    let topic = String::from("topic1");
    let topic_hash = TopicHash::from_raw(topic.clone());

    let topic_config = TopicMeshConfig {
        mesh_n: 4,
        mesh_n_low: 2,
        mesh_n_high: 7,
        mesh_outbound_min: 1,
    };

    // turn off flood publish to test fanout behaviour
    let config = ConfigBuilder::default()
        .flood_publish(false)
        .set_topic_config(topic_hash.clone(), topic_config)
        .build()
        .unwrap();

    let (mut gs, _, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(10) // More than mesh_n
        .topics(vec![topic.clone()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    assert!(
        gs.unsubscribe(&Topic::new(topic.clone())),
        "Should unsubscribe successfully"
    );

    let publish_data = vec![0; 42];
    gs.publish(Topic::new(topic), publish_data).unwrap();

    // Check that fanout size matches the topic-specific mesh_n
    assert_eq!(
        gs.fanout.get(&topic_hashes[0]).unwrap().len(),
        4,
        "Fanout should contain topic-specific mesh_n 4 peers for this topic"
    );

    // Collect publish messages
    let publishes = queues
        .into_values()
        .fold(vec![], |mut collected_publish, mut queue| {
            while !queue.is_empty() {
                if let Some(RpcOut::Publish { message, .. }) = queue.try_pop() {
                    collected_publish.push(message);
                }
            }
            collected_publish
        });

    // Verify sent to topic-specific mesh_n number of peers
    assert_eq!(
        publishes.len(),
        4,
        "Should send a publish message to topic-specific mesh_n 4 fanout peers"
    );
}

#[test]
fn test_publish_message_with_default_transmit_size_config() {
    let topic = Topic::new("test");
    let topic_hash = topic.hash();

    let config = ConfigBuilder::default()
        .max_transmit_size_for_topic(Config::default_max_transmit_size(), topic_hash.clone())
        .validation_mode(ValidationMode::Strict)
        .build()
        .unwrap();

    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(10)
        .topics(vec!["test".to_string()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let data = vec![0; 1024];

    let result = gs.publish(topic.clone(), data);
    assert!(
        result.is_ok(),
        "Expected successful publish within size limit"
    );
    let msg_id = result.unwrap();
    assert!(
        gs.mcache.get(&msg_id).is_some(),
        "Message should be in cache"
    );
}

#[test]
fn test_publish_large_message_with_default_transmit_size_config() {
    let topic = Topic::new("test");
    let topic_hash = topic.hash();

    let config = ConfigBuilder::default()
        .max_transmit_size_for_topic(Config::default_max_transmit_size(), topic_hash.clone())
        .validation_mode(ValidationMode::Strict)
        .build()
        .unwrap();

    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(10)
        .topics(vec!["test".to_string()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let data = vec![0; Config::default_max_transmit_size() + 1];

    let result = gs.publish(topic.clone(), data);
    assert!(
        matches!(result, Err(PublishError::MessageTooLarge)),
        "Expected MessageTooLarge error for oversized message"
    );
}

#[test]
fn test_publish_message_with_specific_transmit_size_config() {
    let topic = Topic::new("test");
    let topic_hash = topic.hash();

    let max_topic_transmit_size = 2000;
    let config = ConfigBuilder::default()
        .max_transmit_size_for_topic(max_topic_transmit_size, topic_hash.clone())
        .validation_mode(ValidationMode::Strict)
        .build()
        .unwrap();

    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(10)
        .topics(vec!["test".to_string()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let data = vec![0; 1024];

    let result = gs.publish(topic.clone(), data);
    assert!(
        result.is_ok(),
        "Expected successful publish within topic-specific size limit"
    );
    let msg_id = result.unwrap();
    assert!(
        gs.mcache.get(&msg_id).is_some(),
        "Message should be in cache"
    );
}

#[test]
fn test_publish_large_message_with_specific_transmit_size_config() {
    let topic = Topic::new("test");
    let topic_hash = topic.hash();

    let max_topic_transmit_size = 2048;
    let config = ConfigBuilder::default()
        .max_transmit_size_for_topic(max_topic_transmit_size, topic_hash.clone())
        .validation_mode(ValidationMode::Strict)
        .build()
        .unwrap();

    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(10)
        .topics(vec!["test".to_string()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let data = vec![0; 2049];

    let result = gs.publish(topic.clone(), data);
    assert!(
        matches!(result, Err(PublishError::MessageTooLarge)),
        "Expected MessageTooLarge error for oversized message with topic-specific config"
    );
}

#[test]
fn test_validation_error_message_size_too_large_topic_specific() {
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let max_size = 2048;

    let config = ConfigBuilder::default()
        .max_transmit_size_for_topic(max_size, topic_hash.clone())
        .validation_mode(ValidationMode::None)
        .build()
        .unwrap();

    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec![String::from("test")])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let data = vec![0u8; max_size + 1];
    let raw_message = RawMessage {
        source: Some(peers[0]),
        data,
        sequence_number: Some(1u64),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: false,
    };

    gs.on_connection_handler_event(
        peers[0],
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![raw_message],
                subscriptions: vec![],
                control_msgs: vec![],
            },
            invalid_messages: vec![],
        },
    );

    let event = gs.events.pop_front().expect("Event should be generated");
    match event {
        ToSwarm::GenerateEvent(Event::Message {
            propagation_source,
            message_id: _,
            message,
        }) => {
            assert_eq!(propagation_source, peers[0]);
            assert_eq!(message.data.len(), max_size + 1);
        }
        ToSwarm::NotifyHandler { peer_id, .. } => {
            assert_eq!(peer_id, peers[0]);
        }
        _ => panic!("Unexpected event"),
    }

    // Simulate a peer sending a message exceeding the topic-specific max_transmit_size (2048
    // bytes). The codec's max_length is set high to allow encoding/decoding the RPC, while
    // max_transmit_sizes enforces the custom topic limit.
    let mut max_transmit_size_map = HashMap::new();
    max_transmit_size_map.insert(topic_hash, max_size);

    let mut codec = GossipsubCodec::new(
        Config::default_max_transmit_size() * 2,
        ValidationMode::None,
        max_transmit_size_map,
    );
    let mut buf = BytesMut::new();
    let rpc = proto::RPC {
        publish: vec![proto::Message {
            from: Some(peers[0].to_bytes()),
            data: Some(vec![0u8; max_size + 1]),
            seqno: Some(1u64.to_be_bytes().to_vec()),
            topic: topic_hashes[0].to_string(),
            signature: None,
            key: None,
        }],
        subscriptions: vec![],
        control: None,
    };
    codec.encode(rpc, &mut buf).unwrap();

    let decoded = codec.decode(&mut buf).unwrap().unwrap();
    match decoded {
        HandlerEvent::Message {
            rpc,
            invalid_messages,
        } => {
            assert!(
                rpc.messages.is_empty(),
                "No valid messages should be present"
            );
            assert_eq!(invalid_messages.len(), 1, "One message should be invalid");
            let (invalid_msg, error) = &invalid_messages[0];
            assert_eq!(invalid_msg.data.len(), max_size + 1);
            assert_eq!(error, &ValidationError::MessageSizeTooLargeForTopic);
        }
        _ => panic!("Unexpected event"),
    }
}

#[test]
fn test_validation_message_size_within_topic_specific() {
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let max_size = 2048;

    let config = ConfigBuilder::default()
        .max_transmit_size_for_topic(max_size, topic_hash.clone())
        .validation_mode(ValidationMode::None)
        .build()
        .unwrap();

    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec![String::from("test")])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let data = vec![0u8; max_size - 100];
    let raw_message = RawMessage {
        source: Some(peers[0]),
        data,
        sequence_number: Some(1u64),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: false,
    };

    gs.on_connection_handler_event(
        peers[0],
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![raw_message],
                subscriptions: vec![],
                control_msgs: vec![],
            },
            invalid_messages: vec![],
        },
    );

    let event = gs.events.pop_front().expect("Event should be generated");
    match event {
        ToSwarm::GenerateEvent(Event::Message {
            propagation_source,
            message_id: _,
            message,
        }) => {
            assert_eq!(propagation_source, peers[0]);
            assert_eq!(message.data.len(), max_size - 100);
        }
        ToSwarm::NotifyHandler { peer_id, .. } => {
            assert_eq!(peer_id, peers[0]);
        }
        _ => panic!("Unexpected event"),
    }

    // Simulate a peer sending a message within the topic-specific max_transmit_size (2048 bytes).
    // The codec's max_length allows encoding/decoding the RPC, and max_transmit_sizes confirms
    // the message size is acceptable for the topic.
    let mut max_transmit_size_map = HashMap::new();
    max_transmit_size_map.insert(topic_hash, max_size);

    let mut codec = GossipsubCodec::new(
        Config::default_max_transmit_size() * 2,
        ValidationMode::None,
        max_transmit_size_map,
    );
    let mut buf = BytesMut::new();
    let rpc = proto::RPC {
        publish: vec![proto::Message {
            from: Some(peers[0].to_bytes()),
            data: Some(vec![0u8; max_size - 100]),
            seqno: Some(1u64.to_be_bytes().to_vec()),
            topic: topic_hashes[0].to_string(),
            signature: None,
            key: None,
        }],
        subscriptions: vec![],
        control: None,
    };
    codec.encode(rpc, &mut buf).unwrap();

    let decoded = codec.decode(&mut buf).unwrap().unwrap();
    match decoded {
        HandlerEvent::Message {
            rpc,
            invalid_messages,
        } => {
            assert_eq!(rpc.messages.len(), 1, "One valid message should be present");
            assert!(invalid_messages.is_empty(), "No messages should be invalid");
            assert_eq!(rpc.messages[0].data.len(), max_size - 100);
        }
        _ => panic!("Unexpected event"),
    }
}
