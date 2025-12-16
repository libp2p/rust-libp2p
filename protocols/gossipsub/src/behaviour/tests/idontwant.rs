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

/// Test that a node sends IDONTWANT messages to mesh peers
/// that run Gossipsub v1.2.
#[test]
fn sends_idontwant() {
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
        data: vec![12u8; 1024],
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
        3,
        "IDONTWANT was not sent"
    );
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
    let (mut gs, peers, queues, topic_hashes) = DefaultBehaviourTestBuilder::default()
        .peer_no(5)
        .topics(vec![String::from("topic1")])
        .to_subscribe(true)
        .gs_config(Config::default())
        .explicit(1)
        .peer_kind(PeerKind::Gossipsubv1_1)
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
                    if matches!(queue.try_pop(), Some(RpcOut::IDontWant(_)) if peer_id != peers[1])
                    {
                        idontwants += 1;
                    }
                }
                idontwants
            }),
        0,
        "IDONTWANT were sent"
    );
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
                    if let Some(RpcOut::Forward { .. }) = queue.try_pop() {
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
