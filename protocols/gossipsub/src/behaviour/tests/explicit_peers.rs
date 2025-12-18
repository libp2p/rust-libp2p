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

//! Tests for explicit peer handling.

use std::collections::BTreeSet;

use libp2p_identity::PeerId;
use libp2p_swarm::ToSwarm;

use super::{count_control_msgs, disconnect_peer, flush_events, DefaultBehaviourTestBuilder};
use crate::{
    config::{Config, ConfigBuilder},
    types::{Prune, RawMessage, RpcOut, Subscription, SubscriptionAction},
    IdentTopic as Topic,
};

/// tests that a peer added as explicit peer gets connected to
#[test]
fn test_explicit_peer_gets_connected() {
    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(0)
        .topics(Vec::new())
        .to_subscribe(true)
        .create_network();

    // create new peer
    let peer = PeerId::random();

    // add peer as explicit peer
    gs.add_explicit_peer(&peer);

    let num_events = gs
        .events
        .iter()
        .filter(|e| match e {
            ToSwarm::Dial { opts } => opts.get_peer_id() == Some(peer),
            _ => false,
        })
        .count();

    assert_eq!(
        num_events, 1,
        "There was no dial peer event for the explicit peer"
    );
}

#[test]
fn test_explicit_peer_reconnects() {
    let config = ConfigBuilder::default()
        .check_explicit_peers_ticks(2)
        .build()
        .unwrap();
    let (mut gs, others, queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(Vec::new())
        .to_subscribe(true)
        .gs_config(config)
        .create_network();

    let peer = others.first().unwrap();

    // add peer as explicit peer
    gs.add_explicit_peer(peer);

    flush_events(&mut gs, queues);

    // disconnect peer
    disconnect_peer(&mut gs, peer);

    gs.heartbeat();

    // check that no reconnect after first heartbeat since `explicit_peer_ticks == 2`
    assert_eq!(
        gs.events
            .iter()
            .filter(|e| match e {
                ToSwarm::Dial { opts } => opts.get_peer_id() == Some(*peer),
                _ => false,
            })
            .count(),
        0,
        "There was a dial peer event before explicit_peer_ticks heartbeats"
    );

    gs.heartbeat();

    // check that there is a reconnect after second heartbeat
    assert!(
        gs.events
            .iter()
            .filter(|e| match e {
                ToSwarm::Dial { opts } => opts.get_peer_id() == Some(*peer),
                _ => false,
            })
            .count()
            >= 1,
        "There was no dial peer event for the explicit peer"
    );
}

#[test]
fn test_handle_graft_explicit_peer() {
    let (mut gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec![String::from("topic1"), String::from("topic2")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .create_network();

    let peer = peers.first().unwrap();

    gs.handle_graft(peer, topic_hashes.clone());

    // peer got not added to mesh
    assert!(gs.mesh[&topic_hashes[0]].is_empty());
    assert!(gs.mesh[&topic_hashes[1]].is_empty());

    // check prunes
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == peer
            && match m {
                RpcOut::Prune(Prune { topic_hash, .. }) => {
                    topic_hash == &topic_hashes[0] || topic_hash == &topic_hashes[1]
                }
                _ => false,
            }
    });
    assert!(
        control_msgs >= 2,
        "Not enough prunes sent when grafting from explicit peer"
    );
}

#[test]
fn explicit_peers_not_added_to_mesh_on_receiving_subscription() {
    let (gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .create_network();

    // only peer 1 is in the mesh not peer 0 (which is an explicit peer)
    assert_eq!(
        gs.mesh[&topic_hashes[0]],
        vec![peers[1]].into_iter().collect()
    );

    // assert that graft gets created to non-explicit peer
    let (control_msgs, queues) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[1] && matches!(m, RpcOut::Graft { .. })
    });
    assert!(
        control_msgs >= 1,
        "No graft message got created to non-explicit peer"
    );

    // assert that no graft gets created to explicit peer
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[0] && matches!(m, RpcOut::Graft { .. })
    });
    assert_eq!(
        control_msgs, 0,
        "A graft message got created to an explicit peer"
    );
}

#[test]
fn do_not_graft_explicit_peer() {
    let (mut gs, others, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec![String::from("topic")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .create_network();

    gs.heartbeat();

    // mesh stays empty
    assert_eq!(gs.mesh[&topic_hashes[0]], BTreeSet::new());

    // assert that no graft gets created to explicit peer
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &others[0] && matches!(m, RpcOut::Graft { .. })
    });
    assert_eq!(
        control_msgs, 0,
        "A graft message got created to an explicit peer"
    );
}

#[test]
fn do_forward_messages_to_explicit_peers() {
    let (mut gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec![String::from("topic1"), String::from("topic2")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .create_network();

    let local_id = PeerId::random();

    let message = RawMessage {
        source: Some(peers[1]),
        data: vec![12],
        sequence_number: Some(0),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };
    gs.handle_received_message(message.clone(), &local_id);
    assert_eq!(
        queues.into_iter().fold(0, |mut fwds, (peer_id, mut queue)| {
            while !queue.is_empty() {
                if matches!(queue.try_pop(), Some(RpcOut::Forward{message: m, ..}) if peer_id == peers[0] && m.data == message.data) {
                    fwds +=1;
                }
            }
            fwds
        }),
        1,
        "The message did not get forwarded to the explicit peer"
    );
}

#[test]
fn explicit_peers_not_added_to_mesh_on_subscribe() {
    let (mut gs, peers, queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(Vec::new())
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .create_network();

    // create new topic, both peers subscribing to it but we do not subscribe to it
    let topic = Topic::new(String::from("t"));
    let topic_hash = topic.hash();
    for peer in peers.iter().take(2) {
        gs.handle_received_subscriptions(
            &[Subscription {
                action: SubscriptionAction::Subscribe,
                topic_hash: topic_hash.clone(),
            }],
            peer,
        );
    }

    // subscribe now to topic
    gs.subscribe(&topic).unwrap();

    // only peer 1 is in the mesh not peer 0 (which is an explicit peer)
    assert_eq!(gs.mesh[&topic_hash], vec![peers[1]].into_iter().collect());

    // assert that graft gets created to non-explicit peer
    let (control_msgs, queues) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[1] && matches!(m, RpcOut::Graft { .. })
    });
    assert!(
        control_msgs > 0,
        "No graft message got created to non-explicit peer"
    );

    // assert that no graft gets created to explicit peer
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[0] && matches!(m, RpcOut::Graft { .. })
    });
    assert_eq!(
        control_msgs, 0,
        "A graft message got created to an explicit peer"
    );
}

#[test]
fn explicit_peers_not_added_to_mesh_from_fanout_on_subscribe() {
    let (mut gs, peers, queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(Vec::new())
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .create_network();

    // create new topic, both peers subscribing to it but we do not subscribe to it
    let topic = Topic::new(String::from("t"));
    let topic_hash = topic.hash();
    for peer in peers.iter().take(2) {
        gs.handle_received_subscriptions(
            &[Subscription {
                action: SubscriptionAction::Subscribe,
                topic_hash: topic_hash.clone(),
            }],
            peer,
        );
    }

    // we send a message for this topic => this will initialize the fanout
    gs.publish(topic.clone(), vec![1, 2, 3]).unwrap();

    // subscribe now to topic
    gs.subscribe(&topic).unwrap();

    // only peer 1 is in the mesh not peer 0 (which is an explicit peer)
    assert_eq!(gs.mesh[&topic_hash], vec![peers[1]].into_iter().collect());

    // assert that graft gets created to non-explicit peer
    let (control_msgs, queues) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[1] && matches!(m, RpcOut::Graft { .. })
    });
    assert!(
        control_msgs >= 1,
        "No graft message got created to non-explicit peer"
    );

    // assert that no graft gets created to explicit peer
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[0] && matches!(m, RpcOut::Graft { .. })
    });
    assert_eq!(
        control_msgs, 0,
        "A graft message got created to an explicit peer"
    );
}

#[test]
fn no_gossip_gets_sent_to_explicit_peers() {
    let (mut gs, peers, mut queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec![String::from("topic1"), String::from("topic2")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .create_network();

    let local_id = PeerId::random();

    let message = RawMessage {
        source: Some(peers[1]),
        data: vec![],
        sequence_number: Some(0),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };

    // forward the message
    gs.handle_received_message(message, &local_id);

    // simulate multiple gossip calls (for randomness)
    for _ in 0..3 {
        gs.emit_gossip();
    }

    // assert that no gossip gets sent to explicit peer
    let mut receiver_queue = queues.remove(&peers[0]).unwrap();
    let mut gossips = 0;
    while !receiver_queue.is_empty() {
        if let Some(RpcOut::IHave(_)) = receiver_queue.try_pop() {
            gossips += 1;
        }
    }
    assert_eq!(gossips, 0, "Gossip got emitted to explicit peer");
}
