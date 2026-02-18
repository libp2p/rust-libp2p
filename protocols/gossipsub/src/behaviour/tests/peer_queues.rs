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

//! Tests for the peer queues, namely back pressure and penalisation of slow peers.

use std::collections::{BTreeSet, HashSet};

use hashlink::LinkedHashMap;
use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionId, ToSwarm};

use crate::{
    config::ConfigBuilder,
    peer_score::{PeerScoreParams, PeerScoreThresholds},
    queue::Queue,
    transform::DataTransform,
    types::{Message, PeerDetails, PeerKind},
    Behaviour, Event, FailedMessages, IdentTopic as Topic, MessageAuthenticity, PublishError,
    ValidationMode,
};

#[test]
fn test_all_queues_full() {
    let gs_config = ConfigBuilder::default()
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap();

    let mut gs: Behaviour = Behaviour::new(MessageAuthenticity::RandomAuthor, gs_config).unwrap();

    let topic_hash = Topic::new("Test").hash();
    let mut peers = vec![];
    let mut topics = BTreeSet::new();
    topics.insert(topic_hash.clone());

    let peer_id = PeerId::random();
    peers.push(peer_id);
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(1),
            dont_send: LinkedHashMap::new(),
        },
    );

    let publish_data = vec![2; 59];
    let result = gs.publish(topic_hash.clone(), publish_data.clone());
    assert!(result.is_ok());
    let err = gs.publish(topic_hash, publish_data).unwrap_err();
    assert!(matches!(err, PublishError::AllQueuesFull(f) if f == 1));
}

#[test]
fn test_slow_peer_returns_failed_publish() {
    let gs_config = ConfigBuilder::default()
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap();

    let mut gs: Behaviour = Behaviour::new(MessageAuthenticity::RandomAuthor, gs_config).unwrap();

    let topic_hash = Topic::new("Test").hash();
    let mut peers = vec![];
    let mut topics = BTreeSet::new();
    topics.insert(topic_hash.clone());

    let slow_peer_id = PeerId::random();
    peers.push(slow_peer_id);
    let mesh = gs.mesh.entry(topic_hash.clone()).or_default();
    mesh.insert(slow_peer_id);
    gs.connected_peers.insert(
        slow_peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(1),
            dont_send: LinkedHashMap::new(),
        },
    );
    let peer_id = PeerId::random();
    peers.push(peer_id);
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(gs.config.connection_handler_queue_len()),
            dont_send: LinkedHashMap::new(),
        },
    );

    let publish_data = vec![0; 42];
    let _failed_publish = gs.publish(topic_hash.clone(), publish_data.clone());
    let _failed_publish = gs.publish(topic_hash.clone(), publish_data.clone());
    gs.heartbeat();

    let slow_peer_failed_messages = gs
        .events
        .into_iter()
        .find_map(|e| match e {
            ToSwarm::GenerateEvent(Event::SlowPeer {
                peer_id,
                failed_messages,
            }) if peer_id == slow_peer_id => Some(failed_messages),
            _ => None,
        })
        .expect("No SlowPeer event found");

    let failed_messages = FailedMessages {
        priority: 0,
        non_priority: 1,
    };

    assert_eq!(slow_peer_failed_messages, failed_messages);
}

#[test]
fn test_slow_peer_returns_failed_ihave_handling() {
    let gs_config = ConfigBuilder::default()
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap();

    let mut gs: Behaviour = Behaviour::new(MessageAuthenticity::RandomAuthor, gs_config).unwrap();

    let topic_hash = Topic::new("Test").hash();
    let mut peers = vec![];
    let mut topics = BTreeSet::new();
    topics.insert(topic_hash.clone());

    let slow_peer_id = PeerId::random();
    gs.connected_peers.insert(
        slow_peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(1),
            dont_send: LinkedHashMap::new(),
        },
    );
    peers.push(slow_peer_id);
    let mesh = gs.mesh.entry(topic_hash.clone()).or_default();
    mesh.insert(slow_peer_id);

    let peer_id = PeerId::random();
    peers.push(peer_id);
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(gs.config.connection_handler_queue_len()),
            dont_send: LinkedHashMap::new(),
        },
    );

    // First message.
    let publish_data = vec![1; 59];
    let transformed = gs
        .data_transform
        .outbound_transform(&topic_hash, publish_data.clone())
        .unwrap();
    let raw_message = gs
        .build_raw_message(topic_hash.clone(), transformed)
        .unwrap();
    let msg_id = gs.config.message_id(&Message {
        source: raw_message.source,
        data: publish_data,
        sequence_number: raw_message.sequence_number,
        topic: raw_message.topic.clone(),
    });

    gs.handle_ihave(
        &slow_peer_id,
        vec![(topic_hash.clone(), vec![msg_id.clone()])],
    );

    // Second message.
    let publish_data = vec![2; 59];
    let transformed = gs
        .data_transform
        .outbound_transform(&topic_hash, publish_data.clone())
        .unwrap();
    let raw_message = gs
        .build_raw_message(topic_hash.clone(), transformed)
        .unwrap();
    let msg_id = gs.config.message_id(&Message {
        source: raw_message.source,
        data: publish_data,
        sequence_number: raw_message.sequence_number,
        topic: raw_message.topic.clone(),
    });
    gs.handle_ihave(&slow_peer_id, vec![(topic_hash, vec![msg_id.clone()])]);

    gs.heartbeat();

    let slow_peer_failed_messages = gs
        .events
        .into_iter()
        .find_map(|e| match e {
            ToSwarm::GenerateEvent(Event::SlowPeer {
                peer_id,
                failed_messages,
            }) if peer_id == slow_peer_id => Some(failed_messages),
            _ => None,
        })
        .unwrap();

    let failed_messages = FailedMessages {
        priority: 0,
        non_priority: 1,
    };

    assert_eq!(slow_peer_failed_messages, failed_messages);
}

#[test]
fn test_slow_peer_returns_failed_iwant_handling() {
    let gs_config = ConfigBuilder::default()
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap();

    let mut gs: Behaviour = Behaviour::new(MessageAuthenticity::RandomAuthor, gs_config).unwrap();

    let topic_hash = Topic::new("Test").hash();
    let mut peers = vec![];
    let mut topics = BTreeSet::new();
    topics.insert(topic_hash.clone());

    let slow_peer_id = PeerId::random();
    peers.push(slow_peer_id);
    gs.connected_peers.insert(
        slow_peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(1),
            dont_send: LinkedHashMap::new(),
        },
    );
    peers.push(slow_peer_id);
    let mesh = gs.mesh.entry(topic_hash.clone()).or_default();
    mesh.insert(slow_peer_id);

    let peer_id = PeerId::random();
    peers.push(peer_id);
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(gs.config.connection_handler_queue_len()),
            dont_send: LinkedHashMap::new(),
        },
    );

    let publish_data = vec![1; 59];
    let transformed = gs
        .data_transform
        .outbound_transform(&topic_hash, publish_data.clone())
        .unwrap();
    let raw_message = gs
        .build_raw_message(topic_hash.clone(), transformed)
        .unwrap();
    let msg_id = gs.config.message_id(&Message {
        source: raw_message.source,
        data: publish_data,
        sequence_number: raw_message.sequence_number,
        topic: raw_message.topic.clone(),
    });

    gs.mcache.put(&msg_id, raw_message);
    gs.handle_iwant(&slow_peer_id, vec![msg_id.clone(), msg_id]);

    gs.heartbeat();

    let slow_peer_failed_messages = gs
        .events
        .into_iter()
        .find_map(|e| match e {
            ToSwarm::GenerateEvent(Event::SlowPeer {
                peer_id,
                failed_messages,
            }) if peer_id == slow_peer_id => Some(failed_messages),
            _ => None,
        })
        .unwrap();

    let failed_messages = FailedMessages {
        priority: 0,
        non_priority: 1,
    };

    assert_eq!(slow_peer_failed_messages, failed_messages);
}

#[test]
fn test_slow_peer_returns_failed_forward() {
    let gs_config = ConfigBuilder::default()
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap();

    let mut gs: Behaviour = Behaviour::new(MessageAuthenticity::RandomAuthor, gs_config).unwrap();

    let topic_hash = Topic::new("Test").hash();
    let mut peers = vec![];
    let mut topics = BTreeSet::new();
    topics.insert(topic_hash.clone());

    let slow_peer_id = PeerId::random();
    peers.push(slow_peer_id);
    gs.connected_peers.insert(
        slow_peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(1),
            dont_send: LinkedHashMap::new(),
        },
    );
    peers.push(slow_peer_id);
    let mesh = gs.mesh.entry(topic_hash.clone()).or_default();
    mesh.insert(slow_peer_id);

    let peer_id = PeerId::random();
    peers.push(peer_id);
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(gs.config.connection_handler_queue_len()),
            dont_send: LinkedHashMap::new(),
        },
    );

    let publish_data = vec![1; 59];
    let transformed = gs
        .data_transform
        .outbound_transform(&topic_hash, publish_data.clone())
        .unwrap();
    let raw_message = gs
        .build_raw_message(topic_hash.clone(), transformed)
        .unwrap();
    let msg_id = gs.config.message_id(&Message {
        source: raw_message.source,
        data: publish_data,
        sequence_number: raw_message.sequence_number,
        topic: raw_message.topic.clone(),
    });

    gs.forward_msg(&msg_id, raw_message.clone(), None, HashSet::new());
    gs.forward_msg(&msg_id, raw_message, None, HashSet::new());

    gs.heartbeat();

    let slow_peer_failed_messages = gs
        .events
        .into_iter()
        .find_map(|e| match e {
            ToSwarm::GenerateEvent(Event::SlowPeer {
                peer_id,
                failed_messages,
            }) if peer_id == slow_peer_id => Some(failed_messages),
            _ => None,
        })
        .unwrap();

    let failed_messages = FailedMessages {
        non_priority: 1,
        priority: 0,
    };

    assert_eq!(slow_peer_failed_messages, failed_messages);
}

#[test]
fn test_slow_peer_is_downscored_on_publish() {
    let gs_config = ConfigBuilder::default()
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap();

    let mut gs: Behaviour = Behaviour::new(MessageAuthenticity::RandomAuthor, gs_config).unwrap();
    let slow_peer_params = PeerScoreParams::default();
    gs.with_peer_score(slow_peer_params.clone(), PeerScoreThresholds::default())
        .unwrap();

    let topic_hash = Topic::new("Test").hash();
    let mut peers = vec![];
    let mut topics = BTreeSet::new();
    topics.insert(topic_hash.clone());

    let slow_peer_id = PeerId::random();
    peers.push(slow_peer_id);
    let mesh = gs.mesh.entry(topic_hash.clone()).or_default();
    mesh.insert(slow_peer_id);
    gs.connected_peers.insert(
        slow_peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(1),
            dont_send: LinkedHashMap::new(),
        },
    );
    gs.as_peer_score_mut().add_peer(slow_peer_id);
    let peer_id = PeerId::random();
    peers.push(peer_id);
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_1,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: topics.clone(),
            messages: Queue::new(gs.config.connection_handler_queue_len()),
            dont_send: LinkedHashMap::new(),
        },
    );

    let publish_data = vec![0; 42];
    gs.publish(topic_hash.clone(), publish_data.clone())
        .unwrap();
    let publish_data = vec![2; 59];
    gs.publish(topic_hash.clone(), publish_data).unwrap();
    gs.heartbeat();
    let slow_peer_score = gs.peer_score(&slow_peer_id).unwrap();
    // There should be two penalties for the two failed messages.
    assert_eq!(slow_peer_score, slow_peer_params.slow_peer_weight * 2.0);
}
