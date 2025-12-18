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

//! Tests for GRAFT/PRUNE handling, backoff, and peer exchange.

use std::{
    collections::{HashMap, HashSet},
    thread::sleep,
    time::Duration,
};

use libp2p_identity::PeerId;
use libp2p_swarm::ToSwarm;

use super::{count_control_msgs, disconnect_peer, flush_events, DefaultBehaviourTestBuilder};
use crate::{
    config::{Config, ConfigBuilder},
    topic::TopicHash,
    types::{PeerInfo, Prune, RpcOut},
    IdentTopic as Topic,
};

/// tests that a peer is added to our mesh when we are both subscribed
/// to the same topic
#[test]
fn test_handle_graft_is_subscribed() {
    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .create_network();

    gs.handle_graft(&peers[7], topic_hashes.clone());

    assert!(
        gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
        "Expected peer to have been added to mesh"
    );
}

/// tests that a peer is not added to our mesh when they are subscribed to
/// a topic that we are not
#[test]
fn test_handle_graft_is_not_subscribed() {
    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .create_network();

    gs.handle_graft(
        &peers[7],
        vec![TopicHash::from_raw(String::from("unsubscribed topic"))],
    );

    assert!(
        !gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
        "Expected peer to have been added to mesh"
    );
}

/// tests multiple topics in a single graft message
#[test]
fn test_handle_graft_multiple_topics() {
    let topics: Vec<String> = ["topic1", "topic2", "topic3", "topic4"]
        .iter()
        .map(|&t| String::from(t))
        .collect();

    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(topics)
        .to_subscribe(true)
        .create_network();

    let mut their_topics = topic_hashes.clone();
    // their_topics = [topic1, topic2, topic3]
    // our_topics = [topic1, topic2, topic4]
    their_topics.pop();
    gs.leave(&their_topics[2]);

    gs.handle_graft(&peers[7], their_topics.clone());

    for hash in topic_hashes.iter().take(2) {
        assert!(
            gs.mesh.get(hash).unwrap().contains(&peers[7]),
            "Expected peer to be in the mesh for the first 2 topics"
        );
    }

    assert!(
        !gs.mesh.contains_key(&topic_hashes[2]),
        "Expected the second topic to not be in the mesh"
    );
}

/// tests that a peer is removed from our mesh
#[test]
fn test_handle_prune_peer_in_mesh() {
    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .create_network();

    // insert peer into our mesh for 'topic1'
    gs.mesh
        .insert(topic_hashes[0].clone(), peers.iter().cloned().collect());
    assert!(
        gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
        "Expected peer to be in mesh"
    );

    gs.handle_prune(
        &peers[7],
        topic_hashes
            .iter()
            .map(|h| (h.clone(), vec![], None))
            .collect(),
    );
    assert!(
        !gs.mesh.get(&topic_hashes[0]).unwrap().contains(&peers[7]),
        "Expected peer to be removed from mesh"
    );
}

#[test]
fn test_connect_to_px_peers_on_handle_prune() {
    let config: Config = Config::default();

    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .create_network();

    // handle prune from single peer with px peers

    let mut px = Vec::new();
    // propose more px peers than config.prune_peers()
    for _ in 0..config.prune_peers() + 5 {
        px.push(PeerInfo {
            peer_id: Some(PeerId::random()),
        });
    }

    gs.handle_prune(
        &peers[0],
        vec![(
            topics[0].clone(),
            px.clone(),
            Some(config.prune_backoff().as_secs()),
        )],
    );

    // Check DialPeer events for px peers
    let dials: Vec<_> = gs
        .events
        .iter()
        .filter_map(|e| match e {
            ToSwarm::Dial { opts } => opts.get_peer_id(),
            _ => None,
        })
        .collect();

    // Exactly config.prune_peers() many random peers should be dialled
    assert_eq!(dials.len(), config.prune_peers());

    let dials_set: HashSet<_> = dials.into_iter().collect();

    // No duplicates
    assert_eq!(dials_set.len(), config.prune_peers());

    // all dial peers must be in px
    assert!(dials_set.is_subset(
        &px.iter()
            .map(|i| *i.peer_id.as_ref().unwrap())
            .collect::<HashSet<_>>()
    ));
}

#[test]
fn test_send_px_and_backoff_in_prune() {
    let config: Config = Config::default();

    // build mesh with enough peers for px
    let (mut gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.prune_peers() + 1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .create_network();

    // send prune to peer
    gs.send_graft_prune(
        HashMap::new(),
        vec![(peers[0], vec![topics[0].clone()])]
            .into_iter()
            .collect(),
        HashSet::new(),
    );

    // check prune message
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[0]
            && match m {
                RpcOut::Prune(Prune {
                    topic_hash,
                    peers,
                    backoff,
                }) => {
                    topic_hash == &topics[0] &&
                    peers.len() == config.prune_peers() &&
                    //all peers are different
                    peers.iter().collect::<HashSet<_>>().len() ==
                        config.prune_peers() &&
                    backoff.unwrap() == config.prune_backoff().as_secs()
                }
                _ => false,
            }
    });
    assert_eq!(control_msgs, 1);
}

#[test]
fn test_prune_backoffed_peer_on_graft() {
    let config: Config = Config::default();

    // build mesh with enough peers for px
    let (mut gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.prune_peers() + 1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .create_network();

    // remove peer from mesh and send prune to peer => this adds a backoff for this peer
    gs.mesh.get_mut(&topics[0]).unwrap().remove(&peers[0]);
    gs.send_graft_prune(
        HashMap::new(),
        vec![(peers[0], vec![topics[0].clone()])]
            .into_iter()
            .collect(),
        HashSet::new(),
    );

    // ignore all messages until now
    let queues = flush_events(&mut gs, queues);

    // handle graft
    gs.handle_graft(&peers[0], vec![topics[0].clone()]);

    // check prune message
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[0]
            && match m {
                RpcOut::Prune(Prune {
                    topic_hash,
                    peers,
                    backoff,
                }) => {
                    topic_hash == &topics[0] &&
                    //no px in this case
                    peers.is_empty() &&
                    backoff.unwrap() == config.prune_backoff().as_secs()
                }
                _ => false,
            }
    });
    assert_eq!(control_msgs, 1);
}

#[test]
fn test_do_not_graft_within_backoff_period() {
    let config = ConfigBuilder::default()
        .backoff_slack(1)
        .heartbeat_interval(Duration::from_millis(100))
        .build()
        .unwrap();
    // only one peer => mesh too small and will try to regraft as early as possible
    let (mut gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    // handle prune from peer with backoff of one second
    gs.handle_prune(&peers[0], vec![(topics[0].clone(), Vec::new(), Some(1))]);

    // forget all events until now
    let queues = flush_events(&mut gs, queues);

    // call heartbeat
    gs.heartbeat();

    // Sleep for one second and apply 10 regular heartbeats (interval = 100ms).
    for _ in 0..10 {
        sleep(Duration::from_millis(100));
        gs.heartbeat();
    }

    // Check that no graft got created (we have backoff_slack = 1 therefore one more heartbeat
    // is needed).
    let (control_msgs, queues) =
        count_control_msgs(queues, |_, m| matches!(m, RpcOut::Graft { .. }));
    assert_eq!(
        control_msgs, 0,
        "Graft message created too early within backoff period"
    );

    // Heartbeat one more time this should graft now
    sleep(Duration::from_millis(100));
    gs.heartbeat();

    // check that graft got created
    let (control_msgs, _) = count_control_msgs(queues, |_, m| matches!(m, RpcOut::Graft { .. }));
    assert!(
        control_msgs > 0,
        "No graft message was created after backoff period"
    );
}

#[test]
fn test_do_not_graft_within_default_backoff_period_after_receiving_prune_without_backoff() {
    // set default backoff period to 1 second
    let config = ConfigBuilder::default()
        .prune_backoff(Duration::from_millis(90))
        .backoff_slack(1)
        .heartbeat_interval(Duration::from_millis(100))
        .build()
        .unwrap();
    // only one peer => mesh too small and will try to regraft as early as possible
    let (mut gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    // handle prune from peer without a specified backoff
    gs.handle_prune(&peers[0], vec![(topics[0].clone(), Vec::new(), None)]);

    // forget all events until now
    let queues = flush_events(&mut gs, queues);

    // call heartbeat
    gs.heartbeat();

    // Apply one more heartbeat
    sleep(Duration::from_millis(100));
    gs.heartbeat();

    // Check that no graft got created (we have backoff_slack = 1 therefore one more heartbeat
    // is needed).
    let (control_msgs, queues) =
        count_control_msgs(queues, |_, m| matches!(m, RpcOut::Graft { .. }));
    assert_eq!(
        control_msgs, 0,
        "Graft message created too early within backoff period"
    );

    // Heartbeat one more time this should graft now
    sleep(Duration::from_millis(100));
    gs.heartbeat();

    // check that graft got created
    let (control_msgs, _) = count_control_msgs(queues, |_, m| matches!(m, RpcOut::Graft { .. }));
    assert!(
        control_msgs > 0,
        "No graft message was created after backoff period"
    );
}

#[test]
fn test_unsubscribe_backoff() {
    const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
    let config = ConfigBuilder::default()
        .backoff_slack(1)
        // ensure a prune_backoff > unsubscribe_backoff
        .prune_backoff(Duration::from_secs(5))
        .unsubscribe_backoff(Duration::from_secs(1))
        .heartbeat_interval(HEARTBEAT_INTERVAL)
        .build()
        .unwrap();

    let topic = String::from("test");
    // only one peer => mesh too small and will try to regraft as early as possible
    let (mut gs, _, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec![topic.clone()])
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let _ = gs.unsubscribe(&Topic::new(topic));

    let (control_msgs, queues) = count_control_msgs(queues, |_, m| match m {
        RpcOut::Prune(Prune { backoff, .. }) => backoff == &Some(1),
        _ => false,
    });
    assert_eq!(
        control_msgs, 1,
        "Peer should be pruned with `unsubscribe_backoff`."
    );

    let _ = gs.subscribe(&Topic::new(topics[0].to_string()));

    // forget all events until now
    let queues = flush_events(&mut gs, queues);

    // call heartbeat
    gs.heartbeat();

    // Sleep for one second and apply 10 regular heartbeats (interval = 100ms).
    for _ in 0..10 {
        sleep(HEARTBEAT_INTERVAL);
        gs.heartbeat();
    }

    // Check that no graft got created (we have backoff_slack = 1 therefore one more heartbeat
    // is needed).
    let (control_msgs, queues) =
        count_control_msgs(queues, |_, m| matches!(m, RpcOut::Graft { .. }));
    assert_eq!(
        control_msgs, 0,
        "Graft message created too early within backoff period"
    );

    // Heartbeat one more time this should graft now
    sleep(HEARTBEAT_INTERVAL);
    gs.heartbeat();

    // check that graft got created
    let (control_msgs, _) = count_control_msgs(queues, |_, m| matches!(m, RpcOut::Graft { .. }));
    assert!(
        control_msgs > 0,
        "No graft message was created after backoff period"
    );
}

#[test]
fn test_ignore_graft_from_unknown_topic() {
    let (mut gs, peers, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec![])
        .to_subscribe(true)
        .create_network();

    gs.handle_graft(
        &peers[0],
        vec![TopicHash::from_raw(String::from("unknown"))],
    );

    assert!(
        !gs.mesh
            .contains_key(&TopicHash::from_raw(String::from("unknown"))),
        "Unknown topic should not be added to mesh"
    );
}

#[test]
/// Test nodes that send grafts without subscriptions.
fn test_graft_without_subscribe() {
    // The node should:
    // - Create an empty vector in mesh[topic]
    // - Send subscription request to all peers
    // - run JOIN(topic)

    let topic = String::from("test_subscribe");
    let subscribe_topic = vec![topic.clone()];
    let subscribe_topic_hash = vec![Topic::new(topic.clone()).hash()];
    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(subscribe_topic)
        .to_subscribe(false)
        .create_network();

    assert!(
        gs.mesh.contains_key(&topic_hashes[0]),
        "Subscribe should add a new entry to the mesh[topic] hashmap"
    );

    // The node sends a graft for the subscribe topic.
    gs.handle_graft(&peers[0], subscribe_topic_hash);

    // The node disconnects
    disconnect_peer(&mut gs, &peers[0]);

    // We unsubscribe from the topic.
    let _ = gs.unsubscribe(&Topic::new(topic));
}
