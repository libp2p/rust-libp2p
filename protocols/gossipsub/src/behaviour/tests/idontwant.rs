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

//! Tests for IDONTWANT message handling.

use std::time::Instant;

use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionId, NetworkBehaviour};

use super::DefaultBehaviourTestBuilder;
use crate::{
    config::Config,
    handler::HandlerEvent,
    transform::DataTransform,
    types::{ControlAction, IDontWant, MessageId, PeerKind, RawMessage, RpcIn, RpcOut},
};

/// Helper for IDONTWANT compatibility tests.
/// Sends one large enough message to cross the IDONTWANT threshold in a fixed topology,
/// and return the number of emitted `IDONTWANT` RPCs, so callers can assert behavior by `PeerKind`.
fn send_idontwant_messages(peer_kind: PeerKind) -> usize {
    let (mut gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(5)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .peer_kind(peer_kind)
        .create_network();
    let local_id = PeerId::random();
    let message = RawMessage {
        source: Some(peers[1]),
        data: vec![12u8; 1024],
        sequence_number: Some(0),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };
    gs.handle_received_message(message, &local_id);
    queues
        .into_iter()
        .fold(0, |mut count, (peer_id, mut queue)| {
            while !queue.is_empty() {
                if let Some(RpcOut::IDontWant(_)) = queue.try_pop() {
                    assert_ne!(peer_id, peers[1]);
                    count += 1;
                }
            }
            count
        })
}

/// Test that a node sends IDONTWANT messages to mesh peers
/// that run Gossipsub v1.2
#[test]
fn sends_idontwant_to_supported() {
    let idontwants = send_idontwant_messages(PeerKind::Gossipsubv1_2);
    assert_eq!(idontwants, 3, "Should have sent 3 IDONWANT messages");

    let idontwants = send_idontwant_messages(PeerKind::Gossipsubv1_3);
    assert_eq!(idontwants, 3, "Should have sent 3 IDONWANT messages");
}

#[test]
fn doesnt_sends_idontwant_for_lower_message_size() {
    let (mut gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(5)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .peer_kind(PeerKind::Gossipsubv1_2)
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
        queues
            .into_iter()
            .fold(0, |mut idontwants, (peer_id, mut queue)| {
                while !queue.is_empty() {
                    if let Some(RpcOut::IDontWant(_)) = queue.try_pop() {
                        assert_ne!(peer_id, peers[1]);
                        idontwants += 1;
                    }
                }
                idontwants
            }),
        0,
        "IDONTWANT was sent"
    );
}

/// Test that a node doesn't send IDONTWANT messages to the mesh peers
/// that don't run Gossipsub v1.2.
#[test]
fn doesnt_send_idontwant() {
    let idontwants = send_idontwant_messages(PeerKind::Gossipsubv1_1);
    assert_eq!(idontwants, 0, "Should have sent 0 IDONWANT messages");

    let idontwants = send_idontwant_messages(PeerKind::Gossipsub);
    assert_eq!(idontwants, 0, "Should have sent 0 IDONWANT messages");
}

/// Test that a node doesn't forward a messages to the mesh peers
/// that sent IDONTWANT.
#[test]
fn doesnt_forward_idontwant() {
    let (mut gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(4)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .peer_kind(PeerKind::Gossipsubv1_2)
        .create_network();

    let local_id = PeerId::random();

    let raw_message = RawMessage {
        source: Some(peers[1]),
        data: vec![12],
        sequence_number: Some(0),
        topic: topic_hashes[0].clone(),
        signature: None,
        key: None,
        validated: true,
    };
    let message = gs
        .data_transform
        .inbound_transform(raw_message.clone())
        .unwrap();
    let message_id = gs.config.message_id(&message);
    let peer = gs.connected_peers.get_mut(&peers[2]).unwrap();
    peer.dont_send.insert(message_id, Instant::now());

    gs.handle_received_message(raw_message.clone(), &local_id);
    assert_eq!(
        queues
            .into_iter()
            .fold(0, |mut fwds, (peer_id, mut queue)| {
                while !queue.is_empty() {
                    if let Some(RpcOut::Publish { .. }) = queue.try_pop() {
                        assert_ne!(peer_id, peers[2]);
                        fwds += 1;
                    }
                }
                fwds
            }),
        2,
        "IDONTWANT was not sent"
    );
}

/// Test that a node parses an
/// IDONTWANT message to the respective peer.
#[test]
fn parses_idontwant() {
    let (mut gs, peers, _queues, _topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(2)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .peer_kind(PeerKind::Gossipsubv1_2)
        .create_network();

    let message_id = MessageId::new(&[0, 1, 2, 3]);
    let rpc = RpcIn {
        messages: vec![],
        subscriptions: vec![],
        #[cfg(feature = "partial-messages")]
        partial_message: None,
        control_msgs: vec![ControlAction::IDontWant(IDontWant {
            message_ids: vec![message_id.clone()],
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
    let peer = gs.connected_peers.get_mut(&peers[1]).unwrap();
    assert!(peer.dont_send.get(&message_id).is_some());
}

/// Test that a node clears stale IDONTWANT messages.
#[test]
fn clear_stale_idontwant() {
    use std::time::Duration;

    let (mut gs, peers, _queues, _topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(4)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .peer_kind(PeerKind::Gossipsubv1_2)
        .create_network();

    let peer = gs.connected_peers.get_mut(&peers[2]).unwrap();
    peer.dont_send
        .insert(MessageId::new(&[1, 2, 3, 4]), Instant::now());
    std::thread::sleep(Duration::from_secs(3));
    gs.heartbeat();
    let peer = gs.connected_peers.get_mut(&peers[2]).unwrap();
    assert!(peer.dont_send.is_empty());
}

/// End-to-end: an IDONTWANT received on the wire is acted on by the receiving node's connection
/// handler — which drains the send queue before the event reaches the behaviour.
#[cfg(feature = "metrics")]
#[tokio::test]
async fn idontwant_received_removes_queued_messages() {
    use std::{task::Poll, time::Duration};

    use futures::{StreamExt, future::poll_fn};
    use libp2p_swarm::Swarm;
    use libp2p_swarm_test::SwarmExt as _;
    use prometheus_client::registry::Registry;

    use crate::{IdentTopic, MessageAcceptance, MessageAuthenticity, ValidationMode};

    const PUBLISHES: usize = 200;
    const PAYLOAD: usize = 20 * 1024;
    /// B's inbound validation queue holds 32 entries; publish in smaller batches.
    const VALIDATION_BATCH: usize = 24;
    /// Forwards that must have been parked in B's queue toward A while A was not polled.
    const MIN_PARKED: usize = 60;
    /// Forwards that must be gone after A's IDONTWANTs have been handled (0 without the fix).
    const MIN_REMOVED: u64 = 70;

    /// Drains a swarm until it has no more events to deliver.
    async fn drain(swarm: &mut Swarm<crate::Behaviour>) {
        poll_fn(|cx| {
            loop {
                match swarm.poll_next_unpin(cx) {
                    Poll::Ready(_) => {}
                    Poll::Pending => return Poll::Ready(()),
                }
            }
        })
        .await;
    }

    /// Shared with the handler: incremented when it drains the queue on a received IDONTWANT.
    fn removed_counter(swarm: &Swarm<crate::Behaviour>) -> u64 {
        swarm
            .behaviour()
            .metrics
            .as_ref()
            .expect("metrics configured")
            .removed_queued_messages
            .get()
    }

    let build = |validate: bool| {
        Swarm::new_ephemeral_tokio(|identity| {
            let peer_id = identity.public().to_peer_id();
            let mut builder = crate::ConfigBuilder::default();
            builder
                .heartbeat_initial_delay(Duration::from_millis(50))
                .heartbeat_interval(Duration::from_millis(100))
                .history_length(10)
                .history_gossip(10)
                .validation_mode(ValidationMode::Permissive);
            if validate {
                builder.validate_messages();
            }
            let mut behaviour = crate::Behaviour::new(
                MessageAuthenticity::Author(peer_id),
                builder.build().unwrap(),
            )
            .unwrap();
            if validate {
                behaviour = behaviour.with_metrics(&mut Registry::default(), Default::default());
            }
            behaviour
        })
    };

    let mut a = build(false);
    let mut b = build(true); // gated validation: controls exactly when B forwards
    let mut c = build(false);
    for swarm in [&mut a, &mut b, &mut c] {
        swarm.listen().with_memory_addr_external().await;
    }
    b.connect(&mut a).await;
    c.connect(&mut b).await;
    c.connect(&mut a).await;
    let a_id = *a.local_peer_id();
    let b_id = *b.local_peer_id();
    let c_id = *c.local_peer_id();

    let topic = IdentTopic::new("idontwant-e2e");
    for swarm in [&mut a, &mut b, &mut c] {
        swarm.behaviour_mut().subscribe(&topic).unwrap();
    }

    // Wait for the meshes: C publishes to {A, B}, B forwards to {A}, A forwards to {B}.
    let meshed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            for swarm in [&mut a, &mut b, &mut c] {
                drain(swarm).await;
            }
            let mesh_has = |s: &Swarm<crate::Behaviour>, peers: &[PeerId]| {
                s.behaviour()
                    .mesh
                    .get(&topic.hash())
                    .is_some_and(|mesh| peers.iter().all(|p| mesh.contains(p)))
            };
            if mesh_has(&a, &[b_id]) && mesh_has(&b, &[a_id, c_id]) && mesh_has(&c, &[a_id, b_id]) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(meshed.is_ok(), "meshes did not form in time");

    // A's swarm is deliberately not polled from here: its connection task's event channel
    // fills after 7 events, the task blocks in send, it stops reading,
    // and B's forwards park in B's send queue toward A. C publishes in batches; B only
    // forwards a batch once we accept it below, keeping B's validation queue from overflowing.
    let payload = vec![7u8; PAYLOAD];
    let mut ids = Vec::with_capacity(PUBLISHES);
    for start in (0..PUBLISHES).step_by(VALIDATION_BATCH) {
        let end = (start + VALIDATION_BATCH).min(PUBLISHES);
        for _ in start..end {
            ids.push(
                c.behaviour_mut()
                    .publish(topic.clone(), payload.clone())
                    .expect("publish"),
            );
        }
        for id in &ids[start..end] {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                drain(&mut b).await;
                if b.behaviour_mut().report_message_validation_result(
                    id,
                    &c_id,
                    MessageAcceptance::Accept,
                ) {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "B never received the published message"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

    let parked = b.behaviour().connected_peers[&a_id]
        .messages
        .non_priority_len();
    assert!(
        parked >= MIN_PARKED,
        "expected parked forwards, queue holds {parked}"
    );
    assert_eq!(
        removed_counter(&b),
        0,
        "nothing should have been removed yet"
    );

    // A now processes everything. It saw each message from C first (C's copies were queued in
    // its channels before B could forward), so its forward excludes C and IDONTWANTs go to B,
    // whose handler drains the parked queue.
    let outcome = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            drain(&mut a).await;
            drain(&mut b).await;
            if removed_counter(&b) >= MIN_REMOVED {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        outcome.is_ok(),
        "the handler never removed the queued messages, counter = {}",
        removed_counter(&b)
    );
}
