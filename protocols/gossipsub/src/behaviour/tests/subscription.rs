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

//! Tests for subscription, unsubscription, and join functionality.

use std::collections::HashMap;

use hashlink::LinkedHashMap;
use libp2p_core::ConnectedPoint;

use super::{flush_events, DefaultBehaviourTestBuilder};
use crate::{
    behaviour::tests::BehaviourTestBuilder,
    subscription_filter::WhitelistSubscriptionFilter,
    transform::IdentityTransform,
    types::{PeerDetails, PeerKind, RpcOut, Subscription, SubscriptionAction},
    IdentTopic as Topic,
};

#[test]
/// Test local node subscribing to a topic
fn test_subscribe() {
    // The node should:
    // - Create an empty vector in mesh[topic]
    // - Send subscription request to all peers
    // - run JOIN(topic)

    let subscribe_topic = vec![String::from("test_subscribe")];
    let (gs, _, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(subscribe_topic)
        .to_subscribe(true)
        .create_network();

    assert!(
        gs.mesh.contains_key(&topic_hashes[0]),
        "Subscribe should add a new entry to the mesh[topic] hashmap"
    );

    // collect all the subscriptions
    let subscriptions = queues
        .into_values()
        .fold(0, |mut collected_subscriptions, mut queue| {
            while !queue.is_empty() {
                if let Some(RpcOut::Subscribe(_)) = queue.try_pop() {
                    collected_subscriptions += 1
                }
            }
            collected_subscriptions
        });

    // we sent a subscribe to all known peers
    assert_eq!(subscriptions, 20);
}

/// Test unsubscribe.
#[test]
fn test_unsubscribe() {
    // Unsubscribe should:
    // - Remove the mesh entry for topic
    // - Send UNSUBSCRIBE to all known peers
    // - Call Leave

    let topic_strings = vec![String::from("topic1"), String::from("topic2")];
    let topics = topic_strings
        .iter()
        .map(|t| Topic::new(t.clone()))
        .collect::<Vec<Topic>>();

    // subscribe to topic_strings
    let (mut gs, _, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(topic_strings)
        .to_subscribe(true)
        .create_network();

    for topic_hash in &topic_hashes {
        assert!(
            gs.connected_peers
                .values()
                .any(|p| p.topics.contains(topic_hash)),
            "Topic_peers contain a topic entry"
        );
        assert!(
            gs.mesh.contains_key(topic_hash),
            "mesh should contain a topic entry"
        );
    }

    // unsubscribe from both topics
    assert!(
        gs.unsubscribe(&topics[0]),
        "should be able to unsubscribe successfully from each topic",
    );
    assert!(
        gs.unsubscribe(&topics[1]),
        "should be able to unsubscribe successfully from each topic",
    );

    // collect all the subscriptions
    let subscriptions = queues
        .into_values()
        .fold(0, |mut collected_subscriptions, mut queue| {
            while !queue.is_empty() {
                if let Some(RpcOut::Subscribe(_)) = queue.try_pop() {
                    collected_subscriptions += 1
                }
            }
            collected_subscriptions
        });

    // we sent a unsubscribe to all known peers, for two topics
    assert_eq!(subscriptions, 40);

    // check we clean up internal structures
    for topic_hash in &topic_hashes {
        assert!(
            !gs.mesh.contains_key(topic_hash),
            "All topics should have been removed from the mesh"
        );
    }
}

/// Test JOIN(topic) functionality.
#[test]
fn test_join() {
    use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
    use libp2p_identity::PeerId;
    use libp2p_swarm::{behaviour::ConnectionEstablished, ConnectionId, NetworkBehaviour};

    use crate::{behaviour::FromSwarm, queue::Queue};

    // The Join function should:
    // - Remove peers from fanout[topic]
    // - Add any fanout[topic] peers to the mesh (up to mesh_n)
    // - Fill up to mesh_n peers from known gossipsub peers in the topic
    // - Send GRAFT messages to all nodes added to the mesh

    // This test is not an isolated unit test, rather it uses higher level,
    // subscribe/unsubscribe to perform the test.

    let topic_strings = vec![String::from("topic1"), String::from("topic2")];
    let topics = topic_strings
        .iter()
        .map(|t| Topic::new(t.clone()))
        .collect::<Vec<Topic>>();

    let (mut gs, _, mut queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(topic_strings)
        .to_subscribe(true)
        .create_network();

    // Flush previous GRAFT messages.
    queues = flush_events(&mut gs, queues);

    // unsubscribe, then call join to invoke functionality
    assert!(
        gs.unsubscribe(&topics[0]),
        "should be able to unsubscribe successfully"
    );
    assert!(
        gs.unsubscribe(&topics[1]),
        "should be able to unsubscribe successfully"
    );

    // re-subscribe - there should be peers associated with the topic
    assert!(
        gs.subscribe(&topics[0]).unwrap(),
        "should be able to subscribe successfully"
    );

    // should have added mesh_n nodes to the mesh
    assert!(
        gs.mesh.get(&topic_hashes[0]).unwrap().len() == 6,
        "Should have added 6 nodes to the mesh"
    );

    fn count_grafts(queues: HashMap<PeerId, Queue>) -> (usize, HashMap<PeerId, Queue>) {
        let mut new_queues = HashMap::new();
        let mut acc = 0;

        for (peer_id, mut queue) in queues.into_iter() {
            while !queue.is_empty() {
                if let Some(RpcOut::Graft(_)) = queue.try_pop() {
                    acc += 1;
                }
            }
            new_queues.insert(peer_id, queue);
        }
        (acc, new_queues)
    }

    // there should be mesh_n GRAFT messages.
    let (graft_messages, mut queues) = count_grafts(queues);

    assert_eq!(
        graft_messages, 6,
        "There should be 6 grafts messages sent to peers"
    );

    // verify fanout nodes
    // add 3 random peers to the fanout[topic1]
    gs.fanout
        .insert(topic_hashes[1].clone(), Default::default());
    let mut new_peers: Vec<PeerId> = vec![];

    for _ in 0..3 {
        let random_peer = PeerId::random();
        // inform the behaviour of a new peer
        let address = "/ip4/127.0.0.1".parse::<Multiaddr>().unwrap();
        gs.handle_established_inbound_connection(
            ConnectionId::new_unchecked(0),
            random_peer,
            &address,
            &address,
        )
        .unwrap();
        let queue = Queue::new(gs.config.connection_handler_queue_len());
        let receiver_queue = queue.clone();
        let connection_id = ConnectionId::new_unchecked(0);
        gs.connected_peers.insert(
            random_peer,
            PeerDetails {
                kind: PeerKind::Floodsub,
                outbound: false,
                connections: vec![connection_id],
                topics: Default::default(),
                messages: queue,
                dont_send: LinkedHashMap::new(),
            },
        );
        queues.insert(random_peer, receiver_queue);

        gs.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
            peer_id: random_peer,
            connection_id,
            endpoint: &ConnectedPoint::Dialer {
                address,
                role_override: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
            failed_addresses: &[],
            other_established: 0,
        }));

        // add the new peer to the fanout
        let fanout_peers = gs.fanout.get_mut(&topic_hashes[1]).unwrap();
        fanout_peers.insert(random_peer);
        new_peers.push(random_peer);
    }

    // subscribe to topic1
    gs.subscribe(&topics[1]).unwrap();

    // the three new peers should have been added, along with 3 more from the pool.
    assert!(
        gs.mesh.get(&topic_hashes[1]).unwrap().len() == 6,
        "Should have added 6 nodes to the mesh"
    );
    let mesh_peers = gs.mesh.get(&topic_hashes[1]).unwrap();
    for new_peer in new_peers {
        assert!(
            mesh_peers.contains(&new_peer),
            "Fanout peer should be included in the mesh"
        );
    }

    // there should now 6 graft messages to be sent
    let (graft_messages, _) = count_grafts(queues);

    assert_eq!(
        graft_messages, 6,
        "There should be 6 grafts messages sent to peers"
    );
}

/// Test the gossipsub NetworkBehaviour peer connection logic.
/// Renamed from test_inject_connected
#[test]
fn test_peer_added_on_connection() {
    let (gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![String::from("topic1"), String::from("topic2")])
        .to_subscribe(true)
        .create_network();

    // check that our subscriptions are sent to each of the peers
    // collect all the SendEvents
    let subscriptions = queues.into_iter().fold(
        HashMap::<libp2p_identity::PeerId, Vec<String>>::new(),
        |mut collected_subscriptions, (peer, mut queue)| {
            while !queue.is_empty() {
                if let Some(RpcOut::Subscribe(topic)) = queue.try_pop() {
                    let mut peer_subs = collected_subscriptions.remove(&peer).unwrap_or_default();
                    peer_subs.push(topic.into_string());
                    collected_subscriptions.insert(peer, peer_subs);
                }
            }
            collected_subscriptions
        },
    );

    // check that there are two subscriptions sent to each peer
    for peer_subs in subscriptions.values() {
        assert!(peer_subs.contains(&String::from("topic1")));
        assert!(peer_subs.contains(&String::from("topic2")));
        assert_eq!(peer_subs.len(), 2);
    }

    // check that there are 20 send events created
    assert_eq!(subscriptions.len(), 20);

    // should add the new peers to `peer_topics` with an empty vec as a gossipsub node
    for peer in peers {
        let peer = gs.connected_peers.get(&peer).unwrap();
        assert!(
            peer.topics == topic_hashes.iter().cloned().collect(),
            "The topics for each node should all topics"
        );
    }
}

/// Test subscription handling
#[test]
fn test_handle_received_subscriptions() {
    use std::collections::BTreeSet;

    use libp2p_identity::PeerId;

    // For every subscription:
    // SUBSCRIBE:   - Add subscribed topic to peer_topics for peer.
    //              - Add peer to topics_peer.
    // UNSUBSCRIBE  - Remove topic from peer_topics for peer.
    //              - Remove peer from topic_peers.

    let topics = ["topic1", "topic2", "topic3", "topic4"]
        .iter()
        .map(|&t| String::from(t))
        .collect();
    let (mut gs, peers, _queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(topics)
        .to_subscribe(false)
        .create_network();

    // The first peer sends 3 subscriptions and 1 unsubscription
    let mut subscriptions = topic_hashes[..3]
        .iter()
        .map(|topic_hash| Subscription {
            action: SubscriptionAction::Subscribe,
            topic_hash: topic_hash.clone(),
        })
        .collect::<Vec<Subscription>>();

    subscriptions.push(Subscription {
        action: SubscriptionAction::Unsubscribe,
        topic_hash: topic_hashes[topic_hashes.len() - 1].clone(),
    });

    let unknown_peer = PeerId::random();
    // process the subscriptions
    // first and second peers send subscriptions
    gs.handle_received_subscriptions(&subscriptions, &peers[0]);
    gs.handle_received_subscriptions(&subscriptions, &peers[1]);
    // unknown peer sends the same subscriptions
    gs.handle_received_subscriptions(&subscriptions, &unknown_peer);

    // verify the result

    let peer = gs.connected_peers.get(&peers[0]).unwrap();
    assert!(
        peer.topics
            == topic_hashes
                .iter()
                .take(3)
                .cloned()
                .collect::<BTreeSet<_>>(),
        "First peer should be subscribed to three topics"
    );
    let peer1 = gs.connected_peers.get(&peers[1]).unwrap();
    assert!(
        peer1.topics
            == topic_hashes
                .iter()
                .take(3)
                .cloned()
                .collect::<BTreeSet<_>>(),
        "Second peer should be subscribed to three topics"
    );

    assert!(
        !gs.connected_peers.contains_key(&unknown_peer),
        "Unknown peer should not have been added"
    );

    for topic_hash in topic_hashes[..3].iter() {
        let topic_peers = gs
            .connected_peers
            .iter()
            .filter(|(_, p)| p.topics.contains(topic_hash))
            .map(|(peer_id, _)| *peer_id)
            .collect::<BTreeSet<PeerId>>();
        assert!(
            topic_peers == peers[..2].iter().cloned().collect(),
            "Two peers should be added to the first three topics"
        );
    }

    // Peer 0 unsubscribes from the first topic

    gs.handle_received_subscriptions(
        &[Subscription {
            action: SubscriptionAction::Unsubscribe,
            topic_hash: topic_hashes[0].clone(),
        }],
        &peers[0],
    );

    let peer = gs.connected_peers.get(&peers[0]).unwrap();
    assert!(
        peer.topics == topic_hashes[1..3].iter().cloned().collect::<BTreeSet<_>>(),
        "Peer should be subscribed to two topics"
    );

    // only gossipsub at the moment
    let topic_peers = gs
        .connected_peers
        .iter()
        .filter(|(_, p)| p.topics.contains(&topic_hashes[0]))
        .map(|(peer_id, _)| *peer_id)
        .collect::<BTreeSet<PeerId>>();

    assert!(
        topic_peers == peers[1..2].iter().cloned().collect(),
        "Only the second peers should be in the first topic"
    );
}

#[test]
fn test_subscribe_to_invalid_topic() {
    use std::collections::HashSet;

    let t1 = Topic::new("t1");
    let t2 = Topic::new("t2");
    let (mut gs, _, _, _) = BehaviourTestBuilder::<IdentityTransform, _>::default()
        .subscription_filter(WhitelistSubscriptionFilter(
            vec![t1.hash()].into_iter().collect::<HashSet<_>>(),
        ))
        .create_network();

    assert!(gs.subscribe(&t1).is_ok());
    assert!(gs.subscribe(&t2).is_err());
}

/// Renamed from test_public_api
#[test]
fn test_subscription_public_api() {
    use std::collections::BTreeSet;

    use crate::topic::TopicHash;

    let (gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(4)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .create_network();
    let peers = peers.into_iter().collect::<BTreeSet<_>>();

    assert_eq!(
        gs.topics().cloned().collect::<Vec<_>>(),
        topic_hashes,
        "Expected topics to match registered topic."
    );

    assert_eq!(
        gs.mesh_peers(&TopicHash::from_raw("topic1"))
            .cloned()
            .collect::<BTreeSet<_>>(),
        peers,
        "Expected peers for a registered topic to contain all peers."
    );

    assert_eq!(
        gs.all_mesh_peers().cloned().collect::<BTreeSet<_>>(),
        peers,
        "Expected all_peers to contain all peers."
    );
}
