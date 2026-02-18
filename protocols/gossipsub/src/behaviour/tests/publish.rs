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

//! Tests for publishing and fanout functionality.

use super::DefaultBehaviourTestBuilder;
use crate::{
    config::{Config, ConfigBuilder},
    topic::TopicHash,
    transform::DataTransform,
    types::RpcOut,
    IdentTopic as Topic,
};

/// Test local node publish to subscribed topic
#[test]
fn test_publish_without_flood_publishing() {
    // node should:
    // - Send publish message to all peers
    // - Insert message into gs.mcache and gs.received

    // turn off flood publish to test old behaviour
    let config = ConfigBuilder::default()
        .flood_publish(false)
        .build()
        .unwrap();

    let publish_topic = String::from("test_publish");
    let (mut gs, _, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![publish_topic.clone()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    assert!(
        gs.mesh.contains_key(&topic_hashes[0]),
        "Subscribe should add a new entry to the mesh[topic] hashmap"
    );

    // all peers should be subscribed to the topic
    assert_eq!(
        gs.connected_peers
            .values()
            .filter(|p| p.topics.contains(&topic_hashes[0]))
            .count(),
        20,
        "Peers should be subscribed to the topic"
    );

    // publish on topic
    let publish_data = vec![0; 42];
    gs.publish(Topic::new(publish_topic), publish_data).unwrap();

    // Collect all publish messages
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

    // Transform the inbound message
    let message = &gs
        .data_transform
        .inbound_transform(
            publishes
                .first()
                .expect("Should contain > 0 entries")
                .clone(),
        )
        .unwrap();

    let msg_id = gs.config.message_id(message);

    let config: Config = Config::default();
    assert_eq!(
        publishes.len(),
        config.mesh_n(),
        "Should send a publish message to at least mesh_n peers"
    );

    assert!(
        gs.mcache.get(&msg_id).is_some(),
        "Message cache should contain published message"
    );
}

/// Test local node publish to unsubscribed topic
#[test]
fn test_fanout() {
    // node should:
    // - Populate fanout peers
    // - Send publish message to fanout peers
    // - Insert message into gs.mcache and gs.received

    // turn off flood publish to test fanout behaviour
    let config = ConfigBuilder::default()
        .flood_publish(false)
        .build()
        .unwrap();

    let fanout_topic = String::from("test_fanout");
    let (mut gs, _, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![fanout_topic.clone()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    assert!(
        gs.mesh.contains_key(&topic_hashes[0]),
        "Subscribe should add a new entry to the mesh[topic] hashmap"
    );
    // Unsubscribe from topic
    assert!(
        gs.unsubscribe(&Topic::new(fanout_topic.clone())),
        "should be able to unsubscribe successfully from topic"
    );

    // Publish on unsubscribed topic
    let publish_data = vec![0; 42];
    gs.publish(Topic::new(fanout_topic.clone()), publish_data)
        .unwrap();

    assert_eq!(
        gs.fanout
            .get(&TopicHash::from_raw(fanout_topic))
            .unwrap()
            .len(),
        gs.config.mesh_n(),
        "Fanout should contain `mesh_n` peers for fanout topic"
    );

    // Collect all publish messages
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

    // Transform the inbound message
    let message = &gs
        .data_transform
        .inbound_transform(
            publishes
                .first()
                .expect("Should contain > 0 entries")
                .clone(),
        )
        .unwrap();

    let msg_id = gs.config.message_id(message);

    assert_eq!(
        publishes.len(),
        gs.config.mesh_n(),
        "Should send a publish message to `mesh_n` fanout peers"
    );

    assert!(
        gs.mcache.get(&msg_id).is_some(),
        "Message cache should contain published message"
    );
}

#[test]
fn test_flood_publish() {
    let config: Config = Config::default();

    let topic = "test";
    // Adds more peers than mesh can hold to test flood publishing
    let (mut gs, _, queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high() + 10)
        .topics(vec![topic.into()])
        .to_subscribe(true)
        .create_network();

    // publish message
    let publish_data = vec![0; 42];
    gs.publish(Topic::new(topic), publish_data).unwrap();

    // Collect all publish messages
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

    // Transform the inbound message
    let message = &gs
        .data_transform
        .inbound_transform(
            publishes
                .first()
                .expect("Should contain > 0 entries")
                .clone(),
        )
        .unwrap();

    let msg_id = gs.config.message_id(message);

    let config: Config = Config::default();
    assert_eq!(
        publishes.len(),
        config.mesh_n_high() + 10,
        "Should send a publish message to all known peers"
    );

    assert!(
        gs.mcache.get(&msg_id).is_some(),
        "Message cache should contain published message"
    );
}
