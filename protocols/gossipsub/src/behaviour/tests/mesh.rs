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

//! Tests for mesh management (addition, subtraction, maintenance).

use std::collections::BTreeSet;

use hashlink::LinkedHashMap;
use libp2p_identity::PeerId;
use libp2p_swarm::ConnectionId;

use super::DefaultBehaviourTestBuilder;
use crate::{
    behaviour::{get_random_peers, Behaviour, MessageAuthenticity},
    config::{Config, ConfigBuilder, ValidationMode},
    queue::Queue,
    types::{PeerDetails, PeerKind},
    IdentTopic as Topic,
};

/// Tests the mesh maintenance addition
#[test]
fn test_mesh_addition() {
    let config: Config = Config::default();

    // Adds mesh_low peers and PRUNE 2 giving us a deficit.
    let (mut gs, peers, _queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n() + 1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .create_network();

    let to_remove_peers = config.mesh_n() + 1 - config.mesh_n_low() - 1;

    for peer in peers.iter().take(to_remove_peers) {
        gs.handle_prune(
            peer,
            topics.iter().map(|h| (h.clone(), vec![], None)).collect(),
        );
    }

    // Verify the pruned peers are removed from the mesh.
    assert_eq!(
        gs.mesh.get(&topics[0]).unwrap().len(),
        config.mesh_n_low() - 1
    );

    // run a heartbeat
    gs.heartbeat();

    // Peers should be added to reach mesh_n
    assert_eq!(gs.mesh.get(&topics[0]).unwrap().len(), config.mesh_n());
}

/// Tests the mesh maintenance subtraction
#[test]
fn test_mesh_subtraction() {
    let config = Config::default();

    // Adds mesh_low peers and PRUNE 2 giving us a deficit.
    let n = config.mesh_n_high() + 10;
    // make all outbound connections so that we allow grafting to all
    let (mut gs, peers, _queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(n)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .outbound(n)
        .create_network();

    // graft all the peers
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    // run a heartbeat
    gs.heartbeat();

    // Peers should be removed to reach mesh_n
    assert_eq!(gs.mesh.get(&topics[0]).unwrap().len(), config.mesh_n());
}

#[test]
fn test_do_not_remove_too_many_outbound_peers() {
    let config = Config::default();

    // Adds mesh_low peers and PRUNE 2 giving us a deficit.
    let n = config.mesh_n_high() + 10;
    // make all outbound connections so that we allow grafting to all
    let (mut gs, peers, _queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(n)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .outbound(n)
        .create_network();

    // graft all the peers
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    // run a heartbeat
    gs.heartbeat();

    // Peers should be removed to reach mesh_n
    assert_eq!(gs.mesh.get(&topics[0]).unwrap().len(), config.mesh_n());
}

/// Test Gossipsub.get_random_peers() function
#[test]
fn test_get_random_peers() {
    // generate a default Config
    let gs_config = ConfigBuilder::default()
        .validation_mode(ValidationMode::Anonymous)
        .build()
        .unwrap();
    // create a gossipsub struct
    let mut gs: Behaviour = Behaviour::new(MessageAuthenticity::Anonymous, gs_config).unwrap();

    // create a topic and fill it with some peers
    let topic_hash = Topic::new("Test").hash();
    let mut peers = vec![];
    let mut topics = BTreeSet::new();
    topics.insert(topic_hash.clone());

    for _ in 0..20 {
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
    }

    let random_peers = get_random_peers(&gs.connected_peers, &topic_hash, 5, |_| true);
    assert_eq!(random_peers.len(), 5, "Expected 5 peers to be returned");
    let random_peers = get_random_peers(&gs.connected_peers, &topic_hash, 30, |_| true);
    assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
    assert!(
        random_peers == peers.iter().cloned().collect(),
        "Expected no shuffling"
    );
    let random_peers = get_random_peers(&gs.connected_peers, &topic_hash, 20, |_| true);
    assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
    assert!(
        random_peers == peers.iter().cloned().collect(),
        "Expected no shuffling"
    );
    let random_peers = get_random_peers(&gs.connected_peers, &topic_hash, 0, |_| true);
    assert!(random_peers.is_empty(), "Expected 0 peers to be returned");
    // test the filter
    let random_peers = get_random_peers(&gs.connected_peers, &topic_hash, 5, |_| false);
    assert!(random_peers.is_empty(), "Expected 0 peers to be returned");
    let random_peers = get_random_peers(&gs.connected_peers, &topic_hash, 10, {
        |peer| peers.contains(peer)
    });
    assert!(random_peers.len() == 10, "Expected 10 peers to be returned");
}
