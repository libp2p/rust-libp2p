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

//! Tests for peer scoring functionality.

use std::{
    collections::{HashMap, HashSet},
    net::Ipv4Addr,
    thread::sleep,
    time::Duration,
};

use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionId, FromSwarm, NetworkBehaviour, ToSwarm};

use super::{
    add_peer, add_peer_with_addr, count_control_msgs, proto_to_message, random_message,
    DefaultBehaviourTestBuilder,
};
use crate::{
    behaviour::{ConnectionEstablished, PortUse},
    config::{Config, ConfigBuilder},
    error::ValidationError,
    handler::HandlerEvent,
    peer_score::{PeerScoreParams, PeerScoreThresholds, TopicScoreParams},
    queue::Queue,
    transform::DataTransform,
    types::{
        ControlAction, IHave, IWant, MessageAcceptance, PeerInfo, Prune, RawMessage, RpcIn, RpcOut,
        Subscription, SubscriptionAction,
    },
    IdentTopic as Topic,
};

#[test]
fn test_prune_negative_scored_peers() {
    let config = Config::default();

    // build mesh with one peer
    let (mut gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((
            PeerScoreParams::default(),
            PeerScoreThresholds::default(),
        )))
        .create_network();

    // add penalty to peer
    gs.as_peer_score_mut().add_penalty(&peers[0], 1);

    // execute heartbeat
    gs.heartbeat();

    // peer should not be in mesh anymore
    assert!(gs.mesh[&topics[0]].is_empty());

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
fn test_dont_graft_to_negative_scored_peers() {
    let config = Config::default();
    // init full mesh
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .scoring(Some((
            PeerScoreParams::default(),
            PeerScoreThresholds::default(),
        )))
        .create_network();

    // add two additional peers that will not be part of the mesh
    let (p1, _queue1) = add_peer(&mut gs, &topics, false, false);
    let (p2, _queue2) = add_peer(&mut gs, &topics, false, false);

    // reduce score of p1 to negative
    gs.as_peer_score_mut().add_penalty(&p1, 1);

    // handle prunes of all other peers
    for p in peers {
        gs.handle_prune(&p, vec![(topics[0].clone(), Vec::new(), None)]);
    }

    // heartbeat
    gs.heartbeat();

    // assert that mesh only contains p2
    assert_eq!(gs.mesh.get(&topics[0]).unwrap().len(), 1);
    assert!(gs.mesh.get(&topics[0]).unwrap().contains(&p2));
}

/// Note that in this test also without a penalty the px would be ignored because of the
/// acceptPXThreshold, but the spec still explicitly states the rule that px from negative
/// peers should get ignored, therefore we test it here.
#[test]
fn test_ignore_px_from_negative_scored_peer() {
    let config = Config::default();

    // build mesh with one peer
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .scoring(Some((
            PeerScoreParams::default(),
            PeerScoreThresholds::default(),
        )))
        .create_network();

    // penalize peer
    gs.as_peer_score_mut().add_penalty(&peers[0], 1);

    // handle prune from single peer with px peers
    let px = vec![PeerInfo {
        peer_id: Some(PeerId::random()),
    }];

    gs.handle_prune(
        &peers[0],
        vec![(
            topics[0].clone(),
            px,
            Some(config.prune_backoff().as_secs()),
        )],
    );

    // assert no dials
    assert_eq!(
        gs.events
            .iter()
            .filter(|e| matches!(e, ToSwarm::Dial { .. }))
            .count(),
        0
    );
}

#[test]
fn test_only_send_nonnegative_scoring_peers_in_px() {
    let config = ConfigBuilder::default()
        .prune_peers(16)
        .do_px()
        .build()
        .unwrap();

    // Build mesh with three peer
    let (mut gs, peers, queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(3)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((
            PeerScoreParams::default(),
            PeerScoreThresholds::default(),
        )))
        .create_network();

    // Penalize first peer
    gs.as_peer_score_mut().add_penalty(&peers[0], 1);

    // Prune second peer
    gs.send_graft_prune(
        HashMap::new(),
        vec![(peers[1], vec![topics[0].clone()])]
            .into_iter()
            .collect(),
        HashSet::new(),
    );

    // Check that px in prune message only contains third peer
    let (control_msgs, _) = count_control_msgs(queues, |peer_id, m| {
        peer_id == &peers[1]
            && match m {
                RpcOut::Prune(Prune {
                    topic_hash,
                    peers: px,
                    ..
                }) => {
                    topic_hash == &topics[0]
                        && px.len() == 1
                        && px[0].peer_id.as_ref().unwrap() == &peers[2]
                }
                _ => false,
            }
    });
    assert_eq!(control_msgs, 1);
}

#[test]
fn test_do_not_gossip_to_peers_below_gossip_threshold() {
    let config = Config::default();
    let peer_score_params = PeerScoreParams::default();
    let peer_score_thresholds = PeerScoreThresholds {
        gossip_threshold: 3.0 * peer_score_params.behaviour_penalty_weight,
        ..PeerScoreThresholds::default()
    };

    // Build full mesh
    let (mut gs, peers, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // Graft all the peer
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    // Add two additional peers that will not be part of the mesh
    let (p1, queue1) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p1, queue1);
    let (p2, queue2) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p2, queue2);

    // Reduce score of p1 below peer_score_thresholds.gossip_threshold
    // note that penalties get squared so two penalties means a score of
    // 4 * peer_score_params.behaviour_penalty_weight.
    gs.as_peer_score_mut().add_penalty(&p1, 2);

    // Reduce score of p2 below 0 but not below peer_score_thresholds.gossip_threshold
    gs.as_peer_score_mut().add_penalty(&p2, 1);

    // Receive message
    let raw_message = RawMessage {
        source: Some(PeerId::random()),
        data: vec![],
        sequence_number: Some(0),
        topic: topics[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };
    gs.handle_received_message(raw_message.clone(), &PeerId::random());

    // Transform the inbound message
    let message = &gs.data_transform.inbound_transform(raw_message).unwrap();

    let msg_id = gs.config.message_id(message);

    // Emit gossip
    gs.emit_gossip();

    // Check that exactly one gossip messages got sent and it got sent to p2
    let (control_msgs, _) = count_control_msgs(queues, |peer, action| match action {
        RpcOut::IHave(IHave {
            topic_hash,
            message_ids,
        }) => {
            if topic_hash == &topics[0] && message_ids.iter().any(|id| id == &msg_id) {
                assert_eq!(peer, &p2);
                true
            } else {
                false
            }
        }
        _ => false,
    });
    assert_eq!(control_msgs, 1);
}

#[test]
fn test_iwant_msg_from_peer_below_gossip_threshold_gets_ignored() {
    let config = Config::default();
    let peer_score_params = PeerScoreParams::default();
    let peer_score_thresholds = PeerScoreThresholds {
        gossip_threshold: 3.0 * peer_score_params.behaviour_penalty_weight,
        ..PeerScoreThresholds::default()
    };

    // Build full mesh
    let (mut gs, peers, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // Graft all the peer
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    // Add two additional peers that will not be part of the mesh
    let (p1, queue1) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p1, queue1);
    let (p2, queue2) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p2, queue2);

    // Reduce score of p1 below peer_score_thresholds.gossip_threshold
    // note that penalties get squared so two penalties means a score of
    // 4 * peer_score_params.behaviour_penalty_weight.
    gs.as_peer_score_mut().add_penalty(&p1, 2);

    // Reduce score of p2 below 0 but not below peer_score_thresholds.gossip_threshold
    gs.as_peer_score_mut().add_penalty(&p2, 1);

    // Receive message
    let raw_message = RawMessage {
        source: Some(PeerId::random()),
        data: vec![],
        sequence_number: Some(0),
        topic: topics[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };
    gs.handle_received_message(raw_message.clone(), &PeerId::random());

    // Transform the inbound message
    let message = &gs.data_transform.inbound_transform(raw_message).unwrap();

    let msg_id = gs.config.message_id(message);

    gs.handle_iwant(&p1, vec![msg_id.clone()]);
    gs.handle_iwant(&p2, vec![msg_id.clone()]);

    // the messages we are sending
    let sent_messages =
        queues
            .into_iter()
            .fold(vec![], |mut collected_messages, (peer_id, mut queue)| {
                while !queue.is_empty() {
                    if let Some(RpcOut::Forward { message, .. }) = queue.try_pop() {
                        collected_messages.push((peer_id, message));
                    }
                }
                collected_messages
            });

    // the message got sent to p2
    assert!(sent_messages
        .iter()
        .map(|(peer_id, msg)| (
            peer_id,
            gs.data_transform.inbound_transform(msg.clone()).unwrap()
        ))
        .any(|(peer_id, msg)| peer_id == &p2 && gs.config.message_id(&msg) == msg_id));
    // the message got not sent to p1
    assert!(sent_messages
        .iter()
        .map(|(peer_id, msg)| (
            peer_id,
            gs.data_transform.inbound_transform(msg.clone()).unwrap()
        ))
        .all(|(peer_id, msg)| !(peer_id == &p1 && gs.config.message_id(&msg) == msg_id)));
}

#[test]
fn test_ihave_msg_from_peer_below_gossip_threshold_gets_ignored() {
    let config = Config::default();
    let peer_score_params = PeerScoreParams::default();
    let peer_score_thresholds = PeerScoreThresholds {
        gossip_threshold: 3.0 * peer_score_params.behaviour_penalty_weight,
        ..PeerScoreThresholds::default()
    };
    // build full mesh
    let (mut gs, peers, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // graft all the peer
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    // add two additional peers that will not be part of the mesh
    let (p1, queue1) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p1, queue1);
    let (p2, queue2) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p2, queue2);

    // reduce score of p1 below peer_score_thresholds.gossip_threshold
    // note that penalties get squared so two penalties means a score of
    // 4 * peer_score_params.behaviour_penalty_weight.
    gs.as_peer_score_mut().add_penalty(&p1, 2);

    // reduce score of p2 below 0 but not below peer_score_thresholds.gossip_threshold
    gs.as_peer_score_mut().add_penalty(&p2, 1);

    // message that other peers have
    let raw_message = RawMessage {
        source: Some(PeerId::random()),
        data: vec![],
        sequence_number: Some(0),
        topic: topics[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };

    // Transform the inbound message
    let message = &gs.data_transform.inbound_transform(raw_message).unwrap();

    let msg_id = gs.config.message_id(message);

    gs.handle_ihave(&p1, vec![(topics[0].clone(), vec![msg_id.clone()])]);
    gs.handle_ihave(&p2, vec![(topics[0].clone(), vec![msg_id.clone()])]);

    // check that we sent exactly one IWANT request to p2
    let (control_msgs, _) = count_control_msgs(queues, |peer, c| match c {
        RpcOut::IWant(IWant { message_ids }) => {
            if message_ids.iter().any(|m| m == &msg_id) {
                assert_eq!(peer, &p2);
                true
            } else {
                false
            }
        }
        _ => false,
    });
    assert_eq!(control_msgs, 1);
}

#[test]
fn test_do_not_publish_to_peer_below_publish_threshold() {
    let config = ConfigBuilder::default()
        .flood_publish(false)
        .build()
        .unwrap();
    let peer_score_params = PeerScoreParams::default();
    let peer_score_thresholds = PeerScoreThresholds {
        gossip_threshold: 0.5 * peer_score_params.behaviour_penalty_weight,
        publish_threshold: 3.0 * peer_score_params.behaviour_penalty_weight,
        ..PeerScoreThresholds::default()
    };

    // build mesh with no peers and no subscribed topics
    let (mut gs, _, mut queues, _) = DefaultBehaviourTestBuilder::default()
        .gs_config(config)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // create a new topic for which we are not subscribed
    let topic = Topic::new("test");
    let topics = vec![topic.hash()];

    // add two additional peers that will be added to the mesh
    let (p1, queue1) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p1, queue1);
    let (p2, queue2) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p2, queue2);

    // reduce score of p1 below peer_score_thresholds.publish_threshold
    // note that penalties get squared so two penalties means a score of
    // 4 * peer_score_params.behaviour_penalty_weight.
    gs.as_peer_score_mut().add_penalty(&p1, 2);

    // reduce score of p2 below 0 but not below peer_score_thresholds.publish_threshold
    gs.as_peer_score_mut().add_penalty(&p2, 1);

    // a heartbeat will remove the peers from the mesh
    gs.heartbeat();

    // publish on topic
    let publish_data = vec![0; 42];
    gs.publish(topic, publish_data).unwrap();

    // Collect all publish messages
    let publishes =
        queues
            .into_iter()
            .fold(vec![], |mut collected_publish, (peer_id, mut queue)| {
                while !queue.is_empty() {
                    if let Some(RpcOut::Publish { message, .. }) = queue.try_pop() {
                        collected_publish.push((peer_id, message));
                    }
                }
                collected_publish
            });

    // assert only published to p2
    assert_eq!(publishes.len(), 1);
    assert_eq!(publishes[0].0, p2);
}

#[test]
fn test_do_not_flood_publish_to_peer_below_publish_threshold() {
    let config = Config::default();
    let peer_score_params = PeerScoreParams::default();
    let peer_score_thresholds = PeerScoreThresholds {
        gossip_threshold: 0.5 * peer_score_params.behaviour_penalty_weight,
        publish_threshold: 3.0 * peer_score_params.behaviour_penalty_weight,
        ..PeerScoreThresholds::default()
    };
    // build mesh with no peers
    let (mut gs, _, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .topics(vec!["test".into()])
        .gs_config(config)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // add two additional peers that will be added to the mesh
    let (p1, queue1) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p1, queue1);
    let (p2, queue2) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p2, queue2);

    // reduce score of p1 below peer_score_thresholds.publish_threshold
    // note that penalties get squared so two penalties means a score of
    // 4 * peer_score_params.behaviour_penalty_weight.
    gs.as_peer_score_mut().add_penalty(&p1, 2);

    // reduce score of p2 below 0 but not below peer_score_thresholds.publish_threshold
    gs.as_peer_score_mut().add_penalty(&p2, 1);

    // a heartbeat will remove the peers from the mesh
    gs.heartbeat();

    // publish on topic
    let publish_data = vec![0; 42];
    gs.publish(Topic::new("test"), publish_data).unwrap();

    // Collect all publish messages
    let publishes =
        queues
            .into_iter()
            .fold(vec![], |mut collected_publish, (peer_id, mut queue)| {
                while !queue.is_empty() {
                    if let Some(RpcOut::Publish { message, .. }) = queue.try_pop() {
                        collected_publish.push((peer_id, message))
                    }
                }
                collected_publish
            });

    // assert only published to p2
    assert_eq!(publishes.len(), 1);
    assert!(publishes[0].0 == p2);
}

#[test]
fn test_ignore_rpc_from_peers_below_graylist_threshold() {
    let config = Config::default();
    let peer_score_params = PeerScoreParams::default();
    let peer_score_thresholds = PeerScoreThresholds {
        gossip_threshold: 0.5 * peer_score_params.behaviour_penalty_weight,
        publish_threshold: 0.5 * peer_score_params.behaviour_penalty_weight,
        graylist_threshold: 3.0 * peer_score_params.behaviour_penalty_weight,
        ..PeerScoreThresholds::default()
    };

    // build mesh with no peers
    let (mut gs, _, _, topics) = DefaultBehaviourTestBuilder::default()
        .topics(vec!["test".into()])
        .gs_config(config.clone())
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // add two additional peers that will be added to the mesh
    let (p1, _queue1) = add_peer(&mut gs, &topics, false, false);
    let (p2, _queue2) = add_peer(&mut gs, &topics, false, false);

    // reduce score of p1 below peer_score_thresholds.graylist_threshold
    // note that penalties get squared so two penalties means a score of
    // 4 * peer_score_params.behaviour_penalty_weight.
    gs.as_peer_score_mut().add_penalty(&p1, 2);

    // reduce score of p2 below publish_threshold but not below graylist_threshold
    gs.as_peer_score_mut().add_penalty(&p2, 1);

    let raw_message1 = RawMessage {
        source: Some(PeerId::random()),
        data: vec![1, 2, 3, 4],
        sequence_number: Some(1u64),
        topic: topics[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };

    let raw_message2 = RawMessage {
        source: Some(PeerId::random()),
        data: vec![1, 2, 3, 4, 5],
        sequence_number: Some(2u64),
        topic: topics[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };

    let raw_message3 = RawMessage {
        source: Some(PeerId::random()),
        data: vec![1, 2, 3, 4, 5, 6],
        sequence_number: Some(3u64),
        topic: topics[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };

    let raw_message4 = RawMessage {
        source: Some(PeerId::random()),
        data: vec![1, 2, 3, 4, 5, 6, 7],
        sequence_number: Some(4u64),
        topic: topics[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };

    // Transform the inbound message
    let message2 = &gs.data_transform.inbound_transform(raw_message2).unwrap();

    // Transform the inbound message
    let message4 = &gs.data_transform.inbound_transform(raw_message4).unwrap();

    let subscription = Subscription {
        action: SubscriptionAction::Subscribe,
        topic_hash: topics[0].clone(),
    };

    let control_action = ControlAction::IHave(IHave {
        topic_hash: topics[0].clone(),
        message_ids: vec![config.message_id(message2)],
    });

    // clear events
    gs.events.clear();

    // receive from p1
    gs.on_connection_handler_event(
        p1,
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![raw_message1],
                subscriptions: vec![subscription.clone()],
                control_msgs: vec![control_action],
            },
            invalid_messages: Vec::new(),
        },
    );

    // only the subscription event gets processed, the rest is dropped
    assert_eq!(gs.events.len(), 1);
    assert!(matches!(
        gs.events[0],
        ToSwarm::GenerateEvent(crate::Event::Subscribed { .. })
    ));

    let control_action = ControlAction::IHave(IHave {
        topic_hash: topics[0].clone(),
        message_ids: vec![config.message_id(message4)],
    });

    // receive from p2
    gs.on_connection_handler_event(
        p2,
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![raw_message3],
                subscriptions: vec![subscription],
                control_msgs: vec![control_action],
            },
            invalid_messages: Vec::new(),
        },
    );

    // events got processed
    assert!(gs.events.len() > 1);
}

#[test]
fn test_ignore_px_from_peers_below_accept_px_threshold() {
    let config = ConfigBuilder::default().prune_peers(16).build().unwrap();
    let peer_score_params = PeerScoreParams::default();
    let peer_score_thresholds = PeerScoreThresholds {
        accept_px_threshold: peer_score_params.app_specific_weight,
        ..PeerScoreThresholds::default()
    };
    // Build mesh with two peers
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // Decrease score of first peer to less than accept_px_threshold
    gs.set_application_score(&peers[0], 0.99);

    // Increase score of second peer to accept_px_threshold
    gs.set_application_score(&peers[1], 1.0);

    // Handle prune from peer peers[0] with px peers
    let px = vec![PeerInfo {
        peer_id: Some(PeerId::random()),
    }];
    gs.handle_prune(
        &peers[0],
        vec![(
            topics[0].clone(),
            px,
            Some(config.prune_backoff().as_secs()),
        )],
    );

    // Assert no dials
    assert_eq!(
        gs.events
            .iter()
            .filter(|e| matches!(e, ToSwarm::Dial { .. }))
            .count(),
        0
    );

    // handle prune from peer peers[1] with px peers
    let px = vec![PeerInfo {
        peer_id: Some(PeerId::random()),
    }];
    gs.handle_prune(
        &peers[1],
        vec![(
            topics[0].clone(),
            px,
            Some(config.prune_backoff().as_secs()),
        )],
    );

    // assert there are dials now
    assert!(
        gs.events
            .iter()
            .filter(|e| matches!(e, ToSwarm::Dial { .. }))
            .count()
            > 0
    );
}

#[test]
fn test_keep_best_scoring_peers_on_oversubscription() {
    let config = ConfigBuilder::default()
        .mesh_n_low(15)
        .mesh_n(30)
        .mesh_n_high(60)
        .retain_scores(29)
        .build()
        .unwrap();

    let mesh_n_high = config.mesh_n_high();

    let (mut gs, peers, _queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(mesh_n_high)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .scoring(Some((
            PeerScoreParams::default(),
            PeerScoreThresholds::default(),
        )))
        .create_network();

    for peer in &peers {
        gs.handle_graft(peer, topics.clone());
    }

    // assign scores to peers equalling their index

    // set random positive scores
    for (index, peer) in peers.iter().enumerate() {
        gs.set_application_score(peer, index as f64);
    }

    assert_eq!(gs.mesh[&topics[0]].len(), mesh_n_high);

    // heartbeat to prune some peers
    gs.heartbeat();

    assert_eq!(gs.mesh[&topics[0]].len(), config.mesh_n());

    // mesh contains retain_scores best peers
    assert!(gs.mesh[&topics[0]].is_superset(
        &peers[(mesh_n_high - config.retain_scores())..]
            .iter()
            .cloned()
            .collect()
    ));
}

#[test]
fn test_scoring_p1() {
    let config = Config::default();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        time_in_mesh_weight: 2.0,
        time_in_mesh_quantum: Duration::from_millis(50),
        time_in_mesh_cap: 10.0,
        topic_weight: 0.7,
        ..TopicScoreParams::default()
    };
    peer_score_params
        .topics
        .insert(topic_hash, topic_params.clone());
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with one peer
    let (mut gs, peers, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    // sleep for 2 times the mesh_quantum
    sleep(topic_params.time_in_mesh_quantum * 2);
    // refresh scores
    gs.as_peer_score_mut().refresh_scores();
    assert!(
        gs.as_peer_score_mut().score_report(&peers[0]).score
            >= 2.0 * topic_params.time_in_mesh_weight * topic_params.topic_weight,
        "score should be at least 2 * time_in_mesh_weight * topic_weight"
    );
    assert!(
        gs.as_peer_score_mut().score_report(&peers[0]).score
            < 3.0 * topic_params.time_in_mesh_weight * topic_params.topic_weight,
        "score should be less than 3 * time_in_mesh_weight * topic_weight"
    );

    // sleep again for 2 times the mesh_quantum
    sleep(topic_params.time_in_mesh_quantum * 2);
    // refresh scores
    gs.as_peer_score_mut().refresh_scores();
    assert!(
        gs.as_peer_score_mut().score_report(&peers[0]).score
            >= 2.0 * topic_params.time_in_mesh_weight * topic_params.topic_weight,
        "score should be at least 4 * time_in_mesh_weight * topic_weight"
    );

    // sleep for enough periods to reach maximum
    sleep(topic_params.time_in_mesh_quantum * (topic_params.time_in_mesh_cap - 3.0) as u32);
    // refresh scores
    gs.as_peer_score_mut().refresh_scores();
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        topic_params.time_in_mesh_cap
            * topic_params.time_in_mesh_weight
            * topic_params.topic_weight,
        "score should be exactly time_in_mesh_cap * time_in_mesh_weight * topic_weight"
    );
}

#[test]
fn test_scoring_p2() {
    let config = Config::default();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        time_in_mesh_weight: 0.0, // deactivate time in mesh
        first_message_deliveries_weight: 2.0,
        first_message_deliveries_cap: 10.0,
        first_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..TopicScoreParams::default()
    };
    peer_score_params
        .topics
        .insert(topic_hash, topic_params.clone());
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with one peer
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    let m1 = random_message(&mut seq, &topics);
    // peer 0 delivers message first
    deliver_message(&mut gs, 0, m1.clone());
    // peer 1 delivers message second
    deliver_message(&mut gs, 1, m1);

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        1.0 * topic_params.first_message_deliveries_weight * topic_params.topic_weight,
        "score should be exactly first_message_deliveries_weight * topic_weight"
    );

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[1]).score,
        0.0,
        "there should be no score for second message deliveries * topic_weight"
    );

    // peer 2 delivers two new messages
    deliver_message(&mut gs, 1, random_message(&mut seq, &topics));
    deliver_message(&mut gs, 1, random_message(&mut seq, &topics));
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[1]).score,
        2.0 * topic_params.first_message_deliveries_weight * topic_params.topic_weight,
        "score should be exactly 2 * first_message_deliveries_weight * topic_weight"
    );

    // test decaying
    gs.as_peer_score_mut().refresh_scores();

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        1.0 * topic_params.first_message_deliveries_decay
            * topic_params.first_message_deliveries_weight
            * topic_params.topic_weight,
        "score should be exactly first_message_deliveries_decay * \
               first_message_deliveries_weight * topic_weight"
    );

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[1]).score,
        2.0 * topic_params.first_message_deliveries_decay
            * topic_params.first_message_deliveries_weight
            * topic_params.topic_weight,
        "score should be exactly 2 * first_message_deliveries_decay * \
               first_message_deliveries_weight * topic_weight"
    );

    // test cap
    for _ in 0..topic_params.first_message_deliveries_cap as u64 {
        deliver_message(&mut gs, 1, random_message(&mut seq, &topics));
    }

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[1]).score,
        topic_params.first_message_deliveries_cap
            * topic_params.first_message_deliveries_weight
            * topic_params.topic_weight,
        "score should be exactly first_message_deliveries_cap * \
               first_message_deliveries_weight * topic_weight"
    );
}

#[test]
fn test_scoring_p3() {
    let config = Config::default();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        time_in_mesh_weight: 0.0,             // deactivate time in mesh
        first_message_deliveries_weight: 0.0, // deactivate first time deliveries
        mesh_message_deliveries_weight: -2.0,
        mesh_message_deliveries_decay: 0.9,
        mesh_message_deliveries_cap: 10.0,
        mesh_message_deliveries_threshold: 5.0,
        mesh_message_deliveries_activation: Duration::from_secs(1),
        mesh_message_deliveries_window: Duration::from_millis(100),
        topic_weight: 0.7,
        ..TopicScoreParams::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with two peers
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    let mut expected_message_deliveries = 0.0;

    // messages used to test window
    let m1 = random_message(&mut seq, &topics);
    let m2 = random_message(&mut seq, &topics);

    // peer 1 delivers m1
    deliver_message(&mut gs, 1, m1.clone());

    // peer 0 delivers two message
    deliver_message(&mut gs, 0, random_message(&mut seq, &topics));
    deliver_message(&mut gs, 0, random_message(&mut seq, &topics));
    expected_message_deliveries += 2.0;

    sleep(Duration::from_millis(60));

    // peer 1 delivers m2
    deliver_message(&mut gs, 1, m2.clone());

    sleep(Duration::from_millis(70));
    // peer 0 delivers m1 and m2 only m2 gets counted
    deliver_message(&mut gs, 0, m1);
    deliver_message(&mut gs, 0, m2);
    expected_message_deliveries += 1.0;

    sleep(Duration::from_millis(900));

    // message deliveries penalties get activated, peer 0 has only delivered 3 messages and
    // therefore gets a penalty
    gs.as_peer_score_mut().refresh_scores();
    expected_message_deliveries *= 0.9; // decay

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        (5f64 - expected_message_deliveries).powi(2) * -2.0 * 0.7
    );

    // peer 0 delivers a lot of messages => message_deliveries should be capped at 10
    for _ in 0..20 {
        deliver_message(&mut gs, 0, random_message(&mut seq, &topics));
    }

    expected_message_deliveries = 10.0;

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);

    // apply 10 decays
    for _ in 0..10 {
        gs.as_peer_score_mut().refresh_scores();
        expected_message_deliveries *= 0.9; // decay
    }

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        (5f64 - expected_message_deliveries).powi(2) * -2.0 * 0.7
    );
}

#[test]
fn test_scoring_p3b() {
    let config = ConfigBuilder::default()
        .prune_backoff(Duration::from_millis(100))
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        time_in_mesh_weight: 0.0,             // deactivate time in mesh
        first_message_deliveries_weight: 0.0, // deactivate first time deliveries
        mesh_message_deliveries_weight: -2.0,
        mesh_message_deliveries_decay: 0.9,
        mesh_message_deliveries_cap: 10.0,
        mesh_message_deliveries_threshold: 5.0,
        mesh_message_deliveries_activation: Duration::from_secs(1),
        mesh_message_deliveries_window: Duration::from_millis(100),
        mesh_failure_penalty_weight: -3.0,
        mesh_failure_penalty_decay: 0.95,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with one peer
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    let mut expected_message_deliveries = 0.0;

    // add some positive score
    gs.as_peer_score_mut()
        .set_application_score(&peers[0], 100.0);

    // peer 0 delivers two message
    deliver_message(&mut gs, 0, random_message(&mut seq, &topics));
    deliver_message(&mut gs, 0, random_message(&mut seq, &topics));
    expected_message_deliveries += 2.0;

    sleep(Duration::from_millis(1050));

    // activation kicks in
    gs.as_peer_score_mut().refresh_scores();
    expected_message_deliveries *= 0.9; // decay

    // prune peer
    gs.handle_prune(&peers[0], vec![(topics[0].clone(), vec![], None)]);

    // wait backoff
    sleep(Duration::from_millis(130));

    // regraft peer
    gs.handle_graft(&peers[0], topics.clone());

    // the score should now consider p3b
    let mut expected_b3 = (5f64 - expected_message_deliveries).powi(2);
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        100.0 + expected_b3 * -3.0 * 0.7
    );

    // we can also add a new p3 to the score

    // peer 0 delivers one message
    deliver_message(&mut gs, 0, random_message(&mut seq, &topics));
    expected_message_deliveries += 1.0;

    sleep(Duration::from_millis(1050));
    gs.as_peer_score_mut().refresh_scores();
    expected_message_deliveries *= 0.9; // decay
    expected_b3 *= 0.95;

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        100.0 + (expected_b3 * -3.0 + (5f64 - expected_message_deliveries).powi(2) * -2.0) * 0.7
    );
}

#[test]
fn test_scoring_p4_valid_message() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with two peers
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    // peer 0 delivers valid message
    let m1 = random_message(&mut seq, &topics);
    deliver_message(&mut gs, 0, m1.clone());

    // Transform the inbound message
    let message1 = &gs.data_transform.inbound_transform(m1).unwrap();

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);

    // message m1 gets validated
    gs.report_message_validation_result(
        &config.message_id(message1),
        &peers[0],
        MessageAcceptance::Accept,
    );

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);
}

#[test]
fn test_scoring_p4_invalid_signature() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with one peer
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;

    // peer 0 delivers message with invalid signature
    let m = random_message(&mut seq, &topics);

    gs.on_connection_handler_event(
        peers[0],
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![],
                subscriptions: vec![],
                control_msgs: vec![],
            },
            invalid_messages: vec![(m, ValidationError::InvalidSignature)],
        },
    );

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        -2.0 * 0.7
    );
}

#[test]
fn test_scoring_p4_message_from_self() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with two peers
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    // peer 0 delivers invalid message from self
    let mut m = random_message(&mut seq, &topics);
    m.source = Some(*gs.publish_config.get_own_id().unwrap());

    deliver_message(&mut gs, 0, m);
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        -2.0 * 0.7
    );
}

#[test]
fn test_scoring_p4_ignored_message() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with two peers
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    // peer 0 delivers ignored message
    let m1 = random_message(&mut seq, &topics);
    deliver_message(&mut gs, 0, m1.clone());

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);

    // Transform the inbound message
    let message1 = &gs.data_transform.inbound_transform(m1).unwrap();

    // message m1 gets ignored
    gs.report_message_validation_result(
        &config.message_id(message1),
        &peers[0],
        MessageAcceptance::Ignore,
    );

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);
}

#[test]
fn test_scoring_p4_application_invalidated_message() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with two peers
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    // peer 0 delivers invalid message
    let m1 = random_message(&mut seq, &topics);
    deliver_message(&mut gs, 0, m1.clone());

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);

    // Transform the inbound message
    let message1 = &gs.data_transform.inbound_transform(m1).unwrap();

    // message m1 gets rejected
    gs.report_message_validation_result(
        &config.message_id(message1),
        &peers[0],
        MessageAcceptance::Reject,
    );

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        -2.0 * 0.7
    );
}

#[test]
fn test_scoring_p4_application_invalid_message_from_two_peers() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with two peers
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    // peer 0 delivers invalid message
    let m1 = random_message(&mut seq, &topics);
    deliver_message(&mut gs, 0, m1.clone());

    // Transform the inbound message
    let message1 = &gs.data_transform.inbound_transform(m1.clone()).unwrap();

    // peer 1 delivers same message
    deliver_message(&mut gs, 1, m1);

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);
    assert_eq!(gs.as_peer_score_mut().score_report(&peers[1]).score, 0.0);

    // message m1 gets rejected
    gs.report_message_validation_result(
        &config.message_id(message1),
        &peers[0],
        MessageAcceptance::Reject,
    );

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        -2.0 * 0.7
    );
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[1]).score,
        -2.0 * 0.7
    );
}

#[test]
fn test_scoring_p4_three_application_invalid_messages() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with one peer
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    // peer 0 delivers two invalid message
    let m1 = random_message(&mut seq, &topics);
    let m2 = random_message(&mut seq, &topics);
    let m3 = random_message(&mut seq, &topics);
    deliver_message(&mut gs, 0, m1.clone());
    deliver_message(&mut gs, 0, m2.clone());
    deliver_message(&mut gs, 0, m3.clone());

    // Transform the inbound message
    let message1 = &gs.data_transform.inbound_transform(m1).unwrap();

    // Transform the inbound message
    let message2 = &gs.data_transform.inbound_transform(m2).unwrap();
    // Transform the inbound message
    let message3 = &gs.data_transform.inbound_transform(m3).unwrap();

    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);

    // messages gets rejected
    gs.report_message_validation_result(
        &config.message_id(message1),
        &peers[0],
        MessageAcceptance::Reject,
    );

    gs.report_message_validation_result(
        &config.message_id(message2),
        &peers[0],
        MessageAcceptance::Reject,
    );

    gs.report_message_validation_result(
        &config.message_id(message3),
        &peers[0],
        MessageAcceptance::Reject,
    );

    // number of invalid messages gets squared
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        9.0 * -2.0 * 0.7
    );
}

#[test]
fn test_scoring_p4_decay() {
    let config = ConfigBuilder::default()
        .validate_messages()
        .build()
        .unwrap();
    let mut peer_score_params = PeerScoreParams::default();
    let topic = Topic::new("test");
    let topic_hash = topic.hash();
    let topic_params = TopicScoreParams {
        // deactivate time in mesh
        time_in_mesh_weight: 0.0,
        // deactivate first time deliveries
        first_message_deliveries_weight: 0.0,
        // deactivate message deliveries
        mesh_message_deliveries_weight: 0.0,
        // deactivate mesh failure penalties
        mesh_failure_penalty_weight: 0.0,
        invalid_message_deliveries_weight: -2.0,
        invalid_message_deliveries_decay: 0.9,
        topic_weight: 0.7,
        ..Default::default()
    };
    peer_score_params.topics.insert(topic_hash, topic_params);
    peer_score_params.app_specific_weight = 1.0;
    let peer_score_thresholds = PeerScoreThresholds::default();

    // build mesh with one peer
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, peer_score_thresholds)))
        .create_network();

    let mut seq = 0;
    let deliver_message = |gs: &mut crate::Behaviour, index: usize, msg: RawMessage| {
        gs.handle_received_message(msg, &peers[index]);
    };

    // peer 0 delivers invalid message
    let m1 = random_message(&mut seq, &topics);
    deliver_message(&mut gs, 0, m1.clone());

    // Transform the inbound message
    let message1 = &gs.data_transform.inbound_transform(m1).unwrap();
    assert_eq!(gs.as_peer_score_mut().score_report(&peers[0]).score, 0.0);

    // message m1 gets rejected
    gs.report_message_validation_result(
        &config.message_id(message1),
        &peers[0],
        MessageAcceptance::Reject,
    );

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        -2.0 * 0.7
    );

    // we decay
    gs.as_peer_score_mut().refresh_scores();

    // the number of invalids gets decayed to 0.9 and then squared in the score
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        0.9 * 0.9 * -2.0 * 0.7
    );
}

#[test]
fn test_scoring_p5() {
    let peer_score_params = PeerScoreParams {
        app_specific_weight: 2.0,
        ..PeerScoreParams::default()
    };

    // build mesh with one peer
    let (mut gs, peers, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .topics(vec!["test".into()])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, PeerScoreThresholds::default())))
        .create_network();

    gs.set_application_score(&peers[0], 1.1);

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        1.1 * 2.0
    );
}

#[test]
fn test_scoring_p6() {
    let peer_score_params = PeerScoreParams {
        ip_colocation_factor_threshold: 5.0,
        ip_colocation_factor_weight: -2.0,
        ..Default::default()
    };

    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(0)
        .topics(vec![])
        .to_subscribe(false)
        .gs_config(Config::default())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, PeerScoreThresholds::default())))
        .create_network();

    // create 5 peers with the same ip
    let addr = Multiaddr::from(Ipv4Addr::new(10, 1, 2, 3));
    let peers = [
        add_peer_with_addr(&mut gs, &[], false, false, addr.clone()).0,
        add_peer_with_addr(&mut gs, &[], false, false, addr.clone()).0,
        add_peer_with_addr(&mut gs, &[], true, false, addr.clone()).0,
        add_peer_with_addr(&mut gs, &[], true, false, addr.clone()).0,
        add_peer_with_addr(&mut gs, &[], true, true, addr.clone()).0,
    ];

    // create 4 other peers with other ip
    let addr2 = Multiaddr::from(Ipv4Addr::new(10, 1, 2, 4));
    let others = [
        add_peer_with_addr(&mut gs, &[], false, false, addr2.clone()).0,
        add_peer_with_addr(&mut gs, &[], false, false, addr2.clone()).0,
        add_peer_with_addr(&mut gs, &[], true, false, addr2.clone()).0,
        add_peer_with_addr(&mut gs, &[], true, false, addr2.clone()).0,
    ];

    // no penalties yet
    for peer in peers.iter().chain(others.iter()) {
        assert_eq!(gs.as_peer_score_mut().score_report(peer).score, 0.0);
    }

    // add additional connection for 3 others with addr
    for id in others.iter().take(3) {
        gs.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
            peer_id: *id,
            connection_id: ConnectionId::new_unchecked(0),
            endpoint: &libp2p_core::ConnectedPoint::Dialer {
                address: addr.clone(),
                role_override: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
            failed_addresses: &[],
            other_established: 0,
        }));
    }

    // penalties apply squared
    for peer in peers.iter().chain(others.iter().take(3)) {
        assert_eq!(gs.as_peer_score_mut().score_report(peer).score, 9.0 * -2.0);
    }
    // fourth other peer still no penalty
    assert_eq!(gs.as_peer_score_mut().score_report(&others[3]).score, 0.0);

    // add additional connection for 3 of the peers to addr2
    for peer in peers.iter().take(3) {
        gs.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
            peer_id: *peer,
            connection_id: ConnectionId::new_unchecked(0),
            endpoint: &libp2p_core::ConnectedPoint::Dialer {
                address: addr2.clone(),
                role_override: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
            failed_addresses: &[],
            other_established: 1,
        }));
    }

    // double penalties for the first three of each
    for peer in peers.iter().take(3).chain(others.iter().take(3)) {
        assert_eq!(
            gs.as_peer_score_mut().score_report(peer).score,
            (9.0 + 4.0) * -2.0
        );
    }

    // single penalties for the rest
    for peer in peers.iter().skip(3) {
        assert_eq!(gs.as_peer_score_mut().score_report(peer).score, 9.0 * -2.0);
    }
    assert_eq!(
        gs.as_peer_score_mut().score_report(&others[3]).score,
        4.0 * -2.0
    );

    // two times same ip doesn't count twice
    gs.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
        peer_id: peers[0],
        connection_id: ConnectionId::new_unchecked(0),
        endpoint: &libp2p_core::ConnectedPoint::Dialer {
            address: addr,
            role_override: Endpoint::Dialer,
            port_use: PortUse::Reuse,
        },
        failed_addresses: &[],
        other_established: 2,
    }));

    // nothing changed
    // double penalties for the first three of each
    for peer in peers.iter().take(3).chain(others.iter().take(3)) {
        assert_eq!(
            gs.as_peer_score_mut().score_report(peer).score,
            (9.0 + 4.0) * -2.0
        );
    }

    // single penalties for the rest
    for peer in peers.iter().skip(3) {
        assert_eq!(gs.as_peer_score_mut().score_report(peer).score, 9.0 * -2.0);
    }
    assert_eq!(
        gs.as_peer_score_mut().score_report(&others[3]).score,
        4.0 * -2.0
    );
}

#[test]
fn test_scoring_p7_grafts_before_backoff() {
    let config = ConfigBuilder::default()
        .prune_backoff(Duration::from_millis(200))
        .graft_flood_threshold(Duration::from_millis(100))
        .build()
        .unwrap();
    let peer_score_params = PeerScoreParams {
        behaviour_penalty_weight: -2.0,
        behaviour_penalty_decay: 0.9,
        ..Default::default()
    };

    let (mut gs, peers, _queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, PeerScoreThresholds::default())))
        .create_network();

    // remove peers from mesh and send prune to them => this adds a backoff for the peers
    for peer in peers.iter().take(2) {
        gs.mesh.get_mut(&topics[0]).unwrap().remove(peer);
        gs.send_graft_prune(
            HashMap::new(),
            HashMap::from([(*peer, vec![topics[0].clone()])]),
            HashSet::new(),
        );
    }

    // wait 50 millisecs
    sleep(Duration::from_millis(50));

    // first peer tries to graft
    gs.handle_graft(&peers[0], vec![topics[0].clone()]);

    // double behaviour penalty for first peer (squared)
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        4.0 * -2.0
    );

    // wait 100 millisecs
    sleep(Duration::from_millis(100));

    // second peer tries to graft
    gs.handle_graft(&peers[1], vec![topics[0].clone()]);

    // single behaviour penalty for second peer
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[1]).score,
        1.0 * -2.0
    );

    // test decay
    gs.as_peer_score_mut().refresh_scores();

    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[0]).score,
        4.0 * 0.9 * 0.9 * -2.0
    );
    assert_eq!(
        gs.as_peer_score_mut().score_report(&peers[1]).score,
        1.0 * 0.9 * 0.9 * -2.0
    );
}

#[test]
fn test_opportunistic_grafting() {
    let config = ConfigBuilder::default()
        .mesh_n_low(3)
        .mesh_n(5)
        .mesh_n_high(7)
        .mesh_outbound_min(0) // deactivate outbound handling
        .opportunistic_graft_ticks(2)
        .opportunistic_graft_peers(2)
        .build()
        .unwrap();
    let peer_score_params = PeerScoreParams {
        app_specific_weight: 1.0,
        ..Default::default()
    };
    let thresholds = PeerScoreThresholds {
        opportunistic_graft_threshold: 2.0,
        ..Default::default()
    };

    let (mut gs, peers, _queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(5)
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .gs_config(config)
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, thresholds)))
        .create_network();

    // fill mesh with 5 peers
    for peer in &peers {
        gs.handle_graft(peer, topics.clone());
    }

    // add additional 5 peers
    let others: Vec<_> = (0..5)
        .map(|_| add_peer(&mut gs, &topics, false, false))
        .collect();

    // currently mesh equals peers
    assert_eq!(gs.mesh[&topics[0]], peers.iter().cloned().collect());

    // give others high scores (but the first two have not high enough scores)
    for (i, peer) in peers.iter().enumerate().take(5) {
        gs.set_application_score(peer, 0.0 + i as f64);
    }

    // set scores for peers in the mesh
    for (i, (peer, _queue)) in others.iter().enumerate().take(5) {
        gs.set_application_score(peer, 0.0 + i as f64);
    }

    // this gives a median of exactly 2.0 => should not apply opportunistic grafting
    gs.heartbeat();
    gs.heartbeat();

    assert_eq!(
        gs.mesh[&topics[0]].len(),
        5,
        "should not apply opportunistic grafting"
    );

    // reduce middle score to 1.0 giving a median of 1.0
    gs.set_application_score(&peers[2], 1.0);

    // opportunistic grafting after two heartbeats

    gs.heartbeat();
    assert_eq!(
        gs.mesh[&topics[0]].len(),
        5,
        "should not apply opportunistic grafting after first tick"
    );

    gs.heartbeat();

    assert_eq!(
        gs.mesh[&topics[0]].len(),
        7,
        "opportunistic grafting should have added 2 peers"
    );

    assert!(
        gs.mesh[&topics[0]].is_superset(&peers.iter().cloned().collect()),
        "old peers are still part of the mesh"
    );

    assert!(
        gs.mesh[&topics[0]].is_disjoint(&others.iter().map(|(p, _)| p).cloned().take(2).collect()),
        "peers below or equal to median should not be added in opportunistic grafting"
    );
}

#[test]
fn test_subscribe_and_graft_with_negative_score() {
    // simulate a communication between two gossipsub instances
    let (mut gs1, _, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .topics(vec!["test".into()])
        .scoring(Some((
            PeerScoreParams::default(),
            PeerScoreThresholds::default(),
        )))
        .create_network();

    let (mut gs2, _, queues, _) = DefaultBehaviourTestBuilder::default().create_network();

    let connection_id = ConnectionId::new_unchecked(0);

    let topic = Topic::new("test");

    let (p2, _queue1) = add_peer(&mut gs1, &Vec::new(), true, false);
    let (p1, _queue2) = add_peer(&mut gs2, &topic_hashes, false, false);

    // add penalty to peer p2
    gs1.as_peer_score_mut().add_penalty(&p2, 1);

    let original_score = gs1.as_peer_score_mut().score_report(&p2).score;

    // subscribe to topic in gs2
    gs2.subscribe(&topic).unwrap();

    let forward_messages_to_p1 = |gs1: &mut crate::Behaviour<_, _>,
                                  p1: PeerId,
                                  p2: PeerId,
                                  connection_id: ConnectionId,
                                  queues: HashMap<PeerId, Queue>|
     -> HashMap<PeerId, Queue> {
        let new_queues = HashMap::new();
        for (peer_id, mut receiver_queue) in queues.into_iter() {
            match receiver_queue.try_pop() {
                Some(rpc) if peer_id == p1 => {
                    gs1.on_connection_handler_event(
                        p2,
                        connection_id,
                        HandlerEvent::Message {
                            rpc: proto_to_message(&rpc.into_protobuf()),
                            invalid_messages: vec![],
                        },
                    );
                }
                _ => {}
            }
        }
        new_queues
    };

    // forward the subscribe message
    let queues = forward_messages_to_p1(&mut gs1, p1, p2, connection_id, queues);

    // heartbeats on both
    gs1.heartbeat();
    gs2.heartbeat();

    // forward messages again
    forward_messages_to_p1(&mut gs1, p1, p2, connection_id, queues);

    // nobody got penalized
    assert!(gs1.as_peer_score_mut().score_report(&p2).score >= original_score);
}
