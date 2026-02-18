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

//! Tests for IHAVE/IWANT gossip message handling.

use std::{collections::HashSet, thread::sleep, time::Duration};

use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionId, NetworkBehaviour};

use super::{
    add_peer, count_control_msgs, flush_events, random_message, DefaultBehaviourTestBuilder,
};
use crate::{
    config::{Config, ConfigBuilder},
    handler::HandlerEvent,
    peer_score::{PeerScoreParams, PeerScoreThresholds},
    topic::TopicHash,
    transform::DataTransform,
    types::{ControlAction, IDontWant, IHave, IWant, MessageId, RawMessage, RpcIn, RpcOut},
};

/// Tests that the correct message is sent when a peer asks for a message in our cache.
#[test]
fn test_handle_iwant_msg_cached() {
    let (mut gs, peers, queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(Vec::new())
        .to_subscribe(true)
        .create_network();

    let raw_message = RawMessage {
        source: Some(peers[11]),
        data: vec![1, 2, 3, 4],
        sequence_number: Some(1u64),
        topic: TopicHash::from_raw("topic"),
        signature: None,
        key: None,
        validated: true,
    };

    // Transform the inbound message
    let message = &gs
        .data_transform
        .inbound_transform(raw_message.clone())
        .unwrap();

    let msg_id = gs.config.message_id(message);
    gs.mcache.put(&msg_id, raw_message);

    gs.handle_iwant(&peers[7], vec![msg_id.clone()]);

    // the messages we are sending
    let sent_messages = queues
        .into_values()
        .fold(vec![], |mut collected_messages, mut queue| {
            while !queue.is_empty() {
                if let Some(RpcOut::Forward { message, .. }) = queue.try_pop() {
                    collected_messages.push(message)
                }
            }
            collected_messages
        });

    assert!(
        sent_messages
            .iter()
            .map(|msg| gs.data_transform.inbound_transform(msg.clone()).unwrap())
            .any(|msg| gs.config.message_id(&msg) == msg_id),
        "Expected the cached message to be sent to an IWANT peer"
    );
}

/// Tests that messages are sent correctly depending on the shifting of the message cache.
#[test]
fn test_handle_iwant_msg_cached_shifted() {
    let (mut gs, peers, mut queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(Vec::new())
        .to_subscribe(true)
        .create_network();

    // perform 10 memshifts and check that it leaves the cache
    for shift in 1..10 {
        let raw_message = RawMessage {
            source: Some(peers[11]),
            data: vec![1, 2, 3, 4],
            sequence_number: Some(shift),
            topic: TopicHash::from_raw("topic"),
            signature: None,
            key: None,
            validated: true,
        };

        // Transform the inbound message
        let message = &gs
            .data_transform
            .inbound_transform(raw_message.clone())
            .unwrap();

        let msg_id = gs.config.message_id(message);
        gs.mcache.put(&msg_id, raw_message);
        for _ in 0..shift {
            gs.mcache.shift();
        }

        gs.handle_iwant(&peers[7], vec![msg_id.clone()]);

        // is the message is being sent?
        let mut message_exists = false;
        queues = queues
            .into_iter()
            .map(|(peer_id, mut queue)| {
                while !queue.is_empty() {
                    if matches!(queue.try_pop(), Some(RpcOut::Forward{message, ..}) if
                        gs.config.message_id(
                            &gs.data_transform
                                .inbound_transform(message.clone())
                                .unwrap(),
                        ) == msg_id)
                    {
                        message_exists = true;
                    }
                }
                (peer_id, queue)
            })
            .collect();
        // default history_length is 5, expect no messages after shift > 5
        if shift < 5 {
            assert!(
                message_exists,
                "Expected the cached message to be sent to an IWANT peer before 5 shifts"
            );
        } else {
            assert!(
                !message_exists,
                "Expected the cached message to not be sent to an IWANT peer after 5 shifts"
            );
        }
    }
}

/// tests that an event is not created when a peers asks for a message not in our cache
#[test]
fn test_handle_iwant_msg_not_cached() {
    let (mut gs, peers, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(Vec::new())
        .to_subscribe(true)
        .create_network();

    let events_before = gs.events.len();
    gs.handle_iwant(&peers[7], vec![MessageId::new(b"unknown id")]);
    let events_after = gs.events.len();

    assert_eq!(
        events_before, events_after,
        "Expected event count to stay the same"
    );
}

#[test]
fn test_handle_iwant_msg_but_already_sent_idontwant() {
    let (mut gs, peers, queues, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(Vec::new())
        .to_subscribe(true)
        .create_network();

    let raw_message = RawMessage {
        source: Some(peers[11]),
        data: vec![1, 2, 3, 4],
        sequence_number: Some(1u64),
        topic: TopicHash::from_raw("topic"),
        signature: None,
        key: None,
        validated: true,
    };

    // Transform the inbound message
    let message = &gs
        .data_transform
        .inbound_transform(raw_message.clone())
        .unwrap();

    let msg_id = gs.config.message_id(message);
    gs.mcache.put(&msg_id, raw_message);

    // Receive IDONTWANT from Peer 1.
    let rpc = RpcIn {
        messages: vec![],
        subscriptions: vec![],
        control_msgs: vec![ControlAction::IDontWant(IDontWant {
            message_ids: vec![msg_id.clone()],
        })],
    };
    gs.on_connection_handler_event(
        peers[1],
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc,
            invalid_messages: vec![],
        },
    );

    // Receive IWANT from Peer 1.
    gs.handle_iwant(&peers[1], vec![msg_id.clone()]);

    // Check that no messages are sent.
    queues.iter().for_each(|(_, receiver_queue)| {
        assert!(receiver_queue.is_empty());
    });
}

/// tests that an event is created when a peer shares that it has a message we want
#[test]
fn test_handle_ihave_subscribed_and_msg_not_cached() {
    let (mut gs, peers, mut queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .create_network();

    gs.handle_ihave(
        &peers[7],
        vec![(topic_hashes[0].clone(), vec![MessageId::new(b"unknown id")])],
    );

    // check that we sent an IWANT request for `unknown id`
    let mut iwant_exists = false;
    let mut receiver_queue = queues.remove(&peers[7]).unwrap();
    while !receiver_queue.is_empty() {
        if let Some(RpcOut::IWant(IWant { message_ids })) = receiver_queue.try_pop() {
            if message_ids
                .iter()
                .any(|m| *m == MessageId::new(b"unknown id"))
            {
                iwant_exists = true;
                break;
            }
        }
    }

    assert!(
        iwant_exists,
        "Expected to send an IWANT control message for unknown message id"
    );
}

/// tests that an event is not created when a peer shares that it has a message that
/// we already have
#[test]
fn test_handle_ihave_subscribed_and_msg_cached() {
    let (mut gs, peers, _, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .create_network();

    let msg_id = MessageId::new(b"known id");

    let events_before = gs.events.len();
    gs.handle_ihave(&peers[7], vec![(topic_hashes[0].clone(), vec![msg_id])]);
    let events_after = gs.events.len();

    assert_eq!(
        events_before, events_after,
        "Expected event count to stay the same"
    )
}

/// test that an event is not created when a peer shares that it has a message in
/// a topic that we are not subscribed to
#[test]
fn test_handle_ihave_not_subscribed() {
    let (mut gs, peers, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(20)
        .topics(vec![])
        .to_subscribe(true)
        .create_network();

    let events_before = gs.events.len();
    gs.handle_ihave(
        &peers[7],
        vec![(
            TopicHash::from_raw(String::from("unsubscribed topic")),
            vec![MessageId::new(b"irrelevant id")],
        )],
    );
    let events_after = gs.events.len();

    assert_eq!(
        events_before, events_after,
        "Expected event count to stay the same"
    )
}

#[test]
fn test_gossip_to_at_least_gossip_lazy_peers() {
    let config: Config = Config::default();

    // add more peers than in mesh to test gossipping
    // by default only mesh_n_low peers will get added to mesh
    let (mut gs, _, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_low() + config.gossip_lazy() + 1)
        .topics(vec!["topic".into()])
        .to_subscribe(true)
        .create_network();

    // receive message
    let raw_message = RawMessage {
        source: Some(PeerId::random()),
        data: vec![],
        sequence_number: Some(0),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };
    gs.handle_received_message(raw_message.clone(), &PeerId::random());

    // emit gossip
    gs.emit_gossip();

    // Transform the inbound message
    let message = &gs.data_transform.inbound_transform(raw_message).unwrap();

    let msg_id = gs.config.message_id(message);

    // check that exactly config.gossip_lazy() many gossip messages were sent.
    let (control_msgs, _) = count_control_msgs(queues, |_, action| match action {
        RpcOut::IHave(IHave {
            topic_hash,
            message_ids,
        }) => topic_hash == &topic_hashes[0] && message_ids.iter().any(|id| id == &msg_id),
        _ => false,
    });
    assert_eq!(control_msgs, config.gossip_lazy());
}

#[test]
fn test_gossip_to_at_most_gossip_factor_peers() {
    let config: Config = Config::default();

    // add a lot of peers
    let m = config.mesh_n_low() + config.gossip_lazy() * (2.0 / config.gossip_factor()) as usize;
    let (mut gs, _, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(m)
        .topics(vec!["topic".into()])
        .to_subscribe(true)
        .create_network();

    // receive message
    let raw_message = RawMessage {
        source: Some(PeerId::random()),
        data: vec![],
        sequence_number: Some(0),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };
    gs.handle_received_message(raw_message.clone(), &PeerId::random());

    // emit gossip
    gs.emit_gossip();

    // Transform the inbound message
    let message = &gs.data_transform.inbound_transform(raw_message).unwrap();

    let msg_id = gs.config.message_id(message);
    // check that exactly config.gossip_lazy() many gossip messages were sent.
    let (control_msgs, _) = count_control_msgs(queues, |_, action| match action {
        RpcOut::IHave(IHave {
            topic_hash,
            message_ids,
        }) => topic_hash == &topic_hashes[0] && message_ids.iter().any(|id| id == &msg_id),
        _ => false,
    });
    assert_eq!(
        control_msgs,
        ((m - config.mesh_n_low()) as f64 * config.gossip_factor()) as usize
    );
}

#[test]
fn test_ignore_too_many_iwants_from_same_peer_for_same_message() {
    let config = Config::default();
    // build gossipsub with full mesh
    let (mut gs, _, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .create_network();

    // add another peer not in the mesh
    let (peer, queue) = add_peer(&mut gs, &topics, false, false);
    queues.insert(peer, queue);

    // receive a message
    let mut seq = 0;
    let m1 = random_message(&mut seq, &topics);

    // Transform the inbound message
    let message1 = &gs.data_transform.inbound_transform(m1.clone()).unwrap();

    let id = config.message_id(message1);

    gs.handle_received_message(m1, &PeerId::random());

    // clear events
    let queues = flush_events(&mut gs, queues);

    // the first gossip_retransimission many iwants return the valid message, all others are
    // ignored.
    for _ in 0..(2 * config.gossip_retransimission() + 10) {
        gs.handle_iwant(&peer, vec![id.clone()]);
    }

    assert_eq!(
        queues.into_values().fold(0, |mut fwds, mut queue| {
            while !queue.is_empty() {
                if let Some(RpcOut::Forward { .. }) = queue.try_pop() {
                    fwds += 1;
                }
            }
            fwds
        }),
        config.gossip_retransimission() as usize,
        "not more then gossip_retransmission many messages get sent back"
    );
}

#[test]
fn test_ignore_too_many_ihaves() {
    let config = ConfigBuilder::default()
        .max_ihave_messages(10)
        .build()
        .unwrap();
    // build gossipsub with full mesh
    let (mut gs, _, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .gs_config(config.clone())
        .create_network();

    // add another peer not in the mesh
    let (peer, queue) = add_peer(&mut gs, &topics, false, false);
    queues.insert(peer, queue);

    // peer has 20 messages
    let mut seq = 0;
    let messages: Vec<_> = (0..20).map(|_| random_message(&mut seq, &topics)).collect();

    // peer sends us one ihave for each message in order
    for raw_message in &messages {
        // Transform the inbound message
        let message = &gs
            .data_transform
            .inbound_transform(raw_message.clone())
            .unwrap();

        gs.handle_ihave(
            &peer,
            vec![(topics[0].clone(), vec![config.message_id(message)])],
        );
    }

    let first_ten: HashSet<_> = messages
        .iter()
        .take(10)
        .map(|msg| gs.data_transform.inbound_transform(msg.clone()).unwrap())
        .map(|m| config.message_id(&m))
        .collect();

    // we send iwant only for the first 10 messages
    let (control_msgs, queues) = count_control_msgs(queues, |p, action| {
        p == &peer
            && matches!(action, RpcOut::IWant(IWant { message_ids }) if message_ids.len() == 1 && first_ten.contains(&message_ids[0]))
    });
    assert_eq!(
        control_msgs, 10,
        "exactly the first ten ihaves should be processed and one iwant for each created"
    );

    // after a heartbeat everything is forgotten
    gs.heartbeat();

    for raw_message in messages[10..].iter() {
        // Transform the inbound message
        let message = &gs
            .data_transform
            .inbound_transform(raw_message.clone())
            .unwrap();

        gs.handle_ihave(
            &peer,
            vec![(topics[0].clone(), vec![config.message_id(message)])],
        );
    }

    // we sent iwant for all 10 messages
    let (control_msgs, _) = count_control_msgs(queues, |p, action| {
        p == &peer
            && matches!(action, RpcOut::IWant(IWant { message_ids }) if message_ids.len() == 1)
    });
    assert_eq!(control_msgs, 10, "all 20 should get sent");
}

#[test]
fn test_ignore_too_many_messages_in_ihave() {
    let config = ConfigBuilder::default()
        .max_ihave_messages(10)
        .max_ihave_length(10)
        .build()
        .unwrap();
    // build gossipsub with full mesh
    let (mut gs, _, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .gs_config(config.clone())
        .create_network();

    // add another peer not in the mesh
    let (peer, queue) = add_peer(&mut gs, &topics, false, false);
    queues.insert(peer, queue);

    // peer has 30 messages
    let mut seq = 0;
    let message_ids: Vec<_> = (0..30)
        .map(|_| random_message(&mut seq, &topics))
        .map(|msg| gs.data_transform.inbound_transform(msg).unwrap())
        .map(|msg| config.message_id(&msg))
        .collect();

    // peer sends us three ihaves
    gs.handle_ihave(&peer, vec![(topics[0].clone(), message_ids[0..8].to_vec())]);
    gs.handle_ihave(
        &peer,
        vec![(topics[0].clone(), message_ids[0..12].to_vec())],
    );
    gs.handle_ihave(
        &peer,
        vec![(topics[0].clone(), message_ids[0..20].to_vec())],
    );

    let first_twelve: HashSet<_> = message_ids.iter().take(12).collect();

    // we send iwant only for the first 10 messages
    let mut sum = 0;
    let (control_msgs, queues) = count_control_msgs(queues, |p, rpc| match rpc {
        RpcOut::IWant(IWant { message_ids }) => {
            p == &peer && {
                assert!(first_twelve.is_superset(&message_ids.iter().collect()));
                sum += message_ids.len();
                true
            }
        }
        _ => false,
    });
    assert_eq!(
        control_msgs, 2,
        "the third ihave should get ignored and no iwant sent"
    );

    assert_eq!(sum, 10, "exactly the first ten ihaves should be processed");

    // after a heartbeat everything is forgotten
    gs.heartbeat();
    gs.handle_ihave(
        &peer,
        vec![(topics[0].clone(), message_ids[20..30].to_vec())],
    );

    // we sent 10 iwant messages ids via a IWANT rpc.
    let mut sum = 0;
    let (control_msgs, _) = count_control_msgs(queues, |p, rpc| match rpc {
        RpcOut::IWant(IWant { message_ids }) => {
            p == &peer && {
                sum += message_ids.len();
                true
            }
        }
        _ => false,
    });
    assert_eq!(control_msgs, 1);
    assert_eq!(sum, 10, "exactly 20 iwants should get sent");
}

#[test]
fn test_limit_number_of_message_ids_inside_ihave() {
    let config = ConfigBuilder::default()
        .max_ihave_messages(10)
        .max_ihave_length(100)
        .build()
        .unwrap();
    // build gossipsub with full mesh
    let (mut gs, peers, mut queues, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(config.mesh_n_high())
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .gs_config(config)
        .create_network();

    // graft to all peers to really fill the mesh with all the peers
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    // add two other peers not in the mesh
    let (p1, queue1) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p1, queue1);
    let (p2, queue2) = add_peer(&mut gs, &topics, false, false);
    queues.insert(p2, queue2);

    // receive 200 messages from another peer
    let mut seq = 0;
    for _ in 0..200 {
        gs.handle_received_message(random_message(&mut seq, &topics), &PeerId::random());
    }

    // emit gossip
    gs.emit_gossip();

    // both peers should have gotten 100 random ihave messages, to assert the randomness, we
    // assert that both have not gotten the same set of messages, but have an intersection
    // (which is the case with very high probability, the probabiltity of failure is < 10^-58).

    let mut ihaves1 = HashSet::new();
    let mut ihaves2 = HashSet::new();

    let (control_msgs, _) = count_control_msgs(queues, |p, action| match action {
        RpcOut::IHave(IHave { message_ids, .. }) => {
            if p == &p1 {
                ihaves1 = message_ids.iter().cloned().collect();
                true
            } else if p == &p2 {
                ihaves2 = message_ids.iter().cloned().collect();
                true
            } else {
                false
            }
        }
        _ => false,
    });
    assert_eq!(
        control_msgs, 2,
        "should have emitted one ihave to p1 and one to p2"
    );

    assert_eq!(
        ihaves1.len(),
        100,
        "should have sent 100 message ids in ihave to p1"
    );
    assert_eq!(
        ihaves2.len(),
        100,
        "should have sent 100 message ids in ihave to p2"
    );
    assert!(
        ihaves1 != ihaves2,
        "should have sent different random messages to p1 and p2 \
        (this may fail with a probability < 10^-58"
    );
    assert!(
        ihaves1.intersection(&ihaves2).count() > 0,
        "should have sent random messages with some common messages to p1 and p2 \
            (this may fail with a probability < 10^-58"
    );
}

#[test]
fn test_iwant_penalties() {
    let config = ConfigBuilder::default()
        .iwant_followup_time(Duration::from_secs(4))
        .build()
        .unwrap();
    let peer_score_params = PeerScoreParams {
        behaviour_penalty_weight: -1.0,
        ..Default::default()
    };

    // fill the mesh
    let (mut gs, peers, _, topics) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec!["test".into()])
        .to_subscribe(false)
        .gs_config(config.clone())
        .explicit(0)
        .outbound(0)
        .scoring(Some((peer_score_params, PeerScoreThresholds::default())))
        .create_network();

    // graft to all peers to really fill the mesh with all the peers
    for peer in peers {
        gs.handle_graft(&peer, topics.clone());
    }

    // add 100 more peers
    let other_peers: Vec<_> = (0..100)
        .map(|_| add_peer(&mut gs, &topics, false, false))
        .collect();

    // each peer sends us an ihave containing each two message ids
    let mut first_messages = Vec::new();
    let mut second_messages = Vec::new();
    let mut seq = 0;
    for (peer, _queue) in &other_peers {
        let msg1 = random_message(&mut seq, &topics);
        let msg2 = random_message(&mut seq, &topics);

        // Decompress the raw message and calculate the message id.
        // Transform the inbound message
        let message1 = &gs.data_transform.inbound_transform(msg1.clone()).unwrap();

        // Transform the inbound message
        let message2 = &gs.data_transform.inbound_transform(msg2.clone()).unwrap();

        first_messages.push(msg1.clone());
        second_messages.push(msg2.clone());
        gs.handle_ihave(
            peer,
            vec![(
                topics[0].clone(),
                vec![config.message_id(message1), config.message_id(message2)],
            )],
        );
    }

    // the peers send us all the first message ids in time
    for (index, (peer, _queue)) in other_peers.iter().enumerate() {
        gs.handle_received_message(first_messages[index].clone(), peer);
    }

    // now we do a heartbeat no penalization should have been applied yet
    gs.heartbeat();

    for (peer, _queue) in &other_peers {
        assert_eq!(gs.as_peer_score_mut().score_report(peer).score, 0.0);
    }

    // receive the first twenty of the other peers then send their response
    for (index, (peer, _queue)) in other_peers.iter().enumerate().take(20) {
        gs.handle_received_message(second_messages[index].clone(), peer);
    }

    // sleep for the promise duration
    sleep(Duration::from_secs(4));

    // now we do a heartbeat to apply penalization
    gs.heartbeat();

    // now we get the second messages from the last 80 peers.
    for (index, (peer, _queue)) in other_peers.iter().enumerate() {
        if index > 19 {
            gs.handle_received_message(second_messages[index].clone(), peer);
        }
    }

    // no further penalizations should get applied
    gs.heartbeat();

    // Only the last 80 peers should be penalized for not responding in time
    let mut not_penalized = 0;
    let mut single_penalized = 0;
    let mut double_penalized = 0;

    for (i, (peer, _queue)) in other_peers.iter().enumerate() {
        let score = gs.as_peer_score_mut().score_report(peer).score;
        if score == 0.0 {
            not_penalized += 1;
        } else if score == -1.0 {
            assert!(i > 9);
            single_penalized += 1;
        } else if score == -4.0 {
            assert!(i > 9);
            double_penalized += 1
        } else {
            println!("{peer}");
            println!("{score}");
            panic!("Invalid score of peer");
        }
    }

    assert_eq!(not_penalized, 20);
    assert_eq!(single_penalized, 80);
    assert_eq!(double_penalized, 0);
}
