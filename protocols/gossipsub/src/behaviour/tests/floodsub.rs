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

//! Tests for floodsub compatibility.

use std::collections::{HashMap, HashSet};

use libp2p_core::Multiaddr;

use super::{add_peer_with_addr_and_kind, count_control_msgs, DefaultBehaviourTestBuilder};
use crate::{
    config::ConfigBuilder,
    types::{PeerKind, Prune, RpcOut},
    IdentTopic as Topic,
};

#[test]
fn test_publish_to_floodsub_peers_without_flood_publish() {
    let config = ConfigBuilder::default()
        .flood_publish(false)
        .build()
        .unwrap();
    let (mut gs, _, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_low() - 1)
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .gs_config(config)
        .create_network();

    // add two floodsub peer, one explicit, one implicit
    let (p1, queue1) = add_peer_with_addr_and_kind(
        &mut gs,
        &topics,
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Floodsub),
    );
    queues.insert(p1, queue1);

    let (p2, queue2) =
        add_peer_with_addr_and_kind(&mut gs, &topics, false, false, Multiaddr::empty(), None);
    queues.insert(p2, queue2);

    // p1 and p2 are not in the mesh
    assert!(!gs.mesh[&topics[0]].contains(&p1) && !gs.mesh[&topics[0]].contains(&p2));

    // publish a message
    let publish_data = vec![0; 42];
    gs.publish(Topic::new("test"), publish_data).unwrap();

    // Collect publish messages to floodsub peers
    let publishes = queues
        .into_iter()
        .fold(0, |mut collected_publish, (peer_id, mut queue)| {
            while !queue.is_empty() {
                if matches!(queue.try_pop(),
            Some(RpcOut::Publish{..}) if peer_id == p1 || peer_id == p2)
                {
                    collected_publish += 1;
                }
            }
            collected_publish
        });

    assert_eq!(
        publishes, 2,
        "Should send a publish message to all floodsub peers"
    );
}

#[test]
fn test_do_not_use_floodsub_in_fanout() {
    let config = ConfigBuilder::default()
        .flood_publish(false)
        .build()
        .unwrap();
    let (mut gs, _, mut queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_low() - 1)
        .topics(Vec::new())
        .to_subscribe(false)
        .gs_config(config)
        .create_network();

    let topic = Topic::new("test");
    let topics = vec![topic.hash()];

    // add two floodsub peer, one explicit, one implicit
    let (p1, queue1) = add_peer_with_addr_and_kind(
        &mut gs,
        &topics,
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Floodsub),
    );

    queues.insert(p1, queue1);
    let (p2, queue2) =
        add_peer_with_addr_and_kind(&mut gs, &topics, false, false, Multiaddr::empty(), None);

    queues.insert(p2, queue2);
    // publish a message
    let publish_data = vec![0; 42];
    gs.publish(Topic::new("test"), publish_data).unwrap();

    // Collect publish messages to floodsub peers
    let publishes = queues
        .into_iter()
        .fold(0, |mut collected_publish, (peer_id, mut queue)| {
            while !queue.is_empty() {
                if matches!(queue.try_pop(),
            Some(RpcOut::Publish{..}) if peer_id == p1 || peer_id == p2)
                {
                    collected_publish += 1;
                }
            }
            collected_publish
        });

    assert_eq!(
        publishes, 2,
        "Should send a publish message to all floodsub peers"
    );

    assert!(
        !gs.fanout[&topics[0]].contains(&p1) && !gs.fanout[&topics[0]].contains(&p2),
        "Floodsub peers are not allowed in fanout"
    );
}

#[test]
fn test_dont_add_floodsub_peers_to_mesh_on_join() {
    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(0)
        .topics(Vec::new())
        .to_subscribe(false)
        .create_network();

    let topic = Topic::new("test");
    let topics = vec![topic.hash()];

    // add two floodsub peer, one explicit, one implicit
    let _p1 = add_peer_with_addr_and_kind(
        &mut gs,
        &topics,
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Floodsub),
    );
    let _p2 = add_peer_with_addr_and_kind(&mut gs, &topics, false, false, Multiaddr::empty(), None);

    gs.join(&topics[0]);

    assert!(
        gs.mesh[&topics[0]].is_empty(),
        "Floodsub peers should not get added to mesh"
    );
}

#[test]
fn test_dont_send_px_to_old_gossipsub_peers() {
    let (mut gs, _, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(0)
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .create_network();

    // add an old gossipsub peer
    let (p1, _queue1) = add_peer_with_addr_and_kind(
        &mut gs,
        &topics,
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Gossipsub),
    );

    // prune the peer
    gs.send_graft_prune(
        HashMap::new(),
        vec![(p1, topics.clone())].into_iter().collect(),
        HashSet::new(),
    );

    // check that prune does not contain px
    let (control_msgs, _) = count_control_msgs(queues, |_, m| match m {
        RpcOut::Prune(Prune { peers: px, .. }) => !px.is_empty(),
        _ => false,
    });
    assert_eq!(control_msgs, 0, "Should not send px to floodsub peers");
}

#[test]
fn test_dont_send_floodsub_peers_in_px() {
    // build mesh with one peer
    let (mut gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .create_network();

    // add two floodsub peers
    let _p1 = add_peer_with_addr_and_kind(
        &mut gs,
        &topics,
        false,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Floodsub),
    );
    let _p2 = add_peer_with_addr_and_kind(&mut gs, &topics, false, false, Multiaddr::empty(), None);

    // prune only mesh node
    gs.send_graft_prune(
        HashMap::new(),
        vec![(peers[0], topics.clone())].into_iter().collect(),
        HashSet::new(),
    );

    // check that px in prune message is empty
    let (control_msgs, _) = count_control_msgs(queues, |_, m| match m {
        RpcOut::Prune(Prune { peers: px, .. }) => !px.is_empty(),
        _ => false,
    });
    assert_eq!(control_msgs, 0, "Should not include floodsub peers in px");
}

#[test]
fn test_dont_add_floodsub_peers_to_mesh_in_heartbeat() {
    let (mut gs, _, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(0)
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .create_network();

    // add two floodsub peer, one explicit, one implicit
    let _p1 = add_peer_with_addr_and_kind(
        &mut gs,
        &topics,
        true,
        false,
        Multiaddr::empty(),
        Some(PeerKind::Floodsub),
    );
    let _p2 = add_peer_with_addr_and_kind(&mut gs, &topics, true, false, Multiaddr::empty(), None);

    gs.heartbeat();

    assert!(
        gs.mesh[&topics[0]].is_empty(),
        "Floodsub peers should not get added to mesh"
    );
}
