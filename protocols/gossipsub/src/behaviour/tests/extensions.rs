//! Tests for the extensions advertisement and the gossipsub v1.4 Large Message
//! Handling capability.

use std::collections::HashMap;

use asynchronous_codec::{Decoder, Encoder};
use bytes::BytesMut;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{ConnectionId, NetworkBehaviour};

use super::DefaultBehaviourTestBuilder;
use crate::{
    IdentTopic as Topic, ValidationMode,
    config::ConfigBuilder,
    handler::HandlerEvent,
    protocol::GossipsubCodec,
    queue::Queue,
    rpc_proto::proto,
    types::{
        ControlAction, Extensions, ImReceiving, LargeMessageFragment, MessageId, Preamble, RpcIn,
        RpcOut,
    },
};

/// Pops messages from a peer's queue until it finds the extensions
/// advertisement sent on connect.
fn advertised_extensions(queue: &mut Queue) -> Extensions {
    std::iter::from_fn(|| queue.try_pop())
        .find_map(|rpc| {
            if let RpcOut::Extensions(extensions) = rpc {
                Some(extensions)
            } else {
                None
            }
        })
        .expect("Extensions message should be sent on connect")
}

/// Verifies that a peer advertising `largeMessageHandling` is tracked as
/// supporting it, independently of any topic subscriptions.
#[test]
fn test_peer_advertised_extensions_are_tracked() {
    let (mut gs, peers, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .create_network();
    let peer_id = peers[0];
    assert_eq!(gs.connected_peers.get(&peer_id).unwrap().extensions, None);

    let extensions = Extensions {
        partial_messages: None,
        large_message_handling: Some(true),
    };
    gs.on_connection_handler_event(
        peer_id,
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: RpcIn {
                messages: vec![],
                subscriptions: vec![],
                control_msgs: vec![ControlAction::Extensions(Some(extensions))],
                large_message_fragments: vec![],
                #[cfg(feature = "partial-messages")]
                partial_message: None,
            },
            invalid_messages: vec![],
        },
    );

    assert_eq!(
        gs.connected_peers.get(&peer_id).unwrap().extensions,
        Some(extensions)
    );
}

/// Verifies that with the config option unset the extensions advertisement
/// does not include the `largeMessageHandling` flag.
#[test]
fn test_large_message_handling_not_advertised_by_default() {
    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default().create_network();
    let peer_id = PeerId::random();
    gs.handle_established_inbound_connection(
        ConnectionId::new_unchecked(0),
        peer_id,
        &Multiaddr::empty(),
        &Multiaddr::empty(),
    )
    .unwrap();

    let mut queue = gs.connected_peers.get(&peer_id).unwrap().messages.clone();
    let extensions = advertised_extensions(&mut queue);
    assert_eq!(extensions.large_message_handling, None);
}

/// Verifies that with the config option set the extensions advertisement
/// includes `largeMessageHandling`.
#[test]
fn test_large_message_handling_advertised_when_enabled() {
    let config = ConfigBuilder::default()
        .large_message_handling(true)
        .build()
        .unwrap();
    let (mut gs, _, _, _) = DefaultBehaviourTestBuilder::default()
        .gs_config(config)
        .create_network();
    let peer_id = PeerId::random();
    gs.handle_established_inbound_connection(
        ConnectionId::new_unchecked(0),
        peer_id,
        &Multiaddr::empty(),
        &Multiaddr::empty(),
    )
    .unwrap();

    let mut queue = gs.connected_peers.get(&peer_id).unwrap().messages.clone();
    let extensions = advertised_extensions(&mut queue);
    assert_eq!(extensions.large_message_handling, Some(true));
}

/// Verifies that an RPC carrying PREAMBLE, IMRECEIVING and large message
/// fragment entries decodes without error and produces no events.
#[test]
fn test_large_message_rpc_decodes_and_produces_no_events() {
    let message_id = MessageId::new(&[1, 2, 3, 4]);
    let topic_hash = Topic::new("large-message-topic").hash();

    let rpc = proto::Rpc {
        publish: vec![],
        subscriptions: vec![],
        control: Some(proto::ControlMessage {
            ihave: vec![],
            iwant: vec![],
            graft: vec![],
            prune: vec![],
            idontwant: vec![],
            extensions: None,
            preamble: vec![proto::ControlPreamble {
                message_id: Some(message_id.0.clone()),
                message_size: Some(1 << 20),
                topic_id: Some(topic_hash.clone().into_string()),
            }],
            imreceiving: vec![proto::ControlImReceiving {
                message_id: Some(message_id.0.clone()),
            }],
        }),
        partial: None,
        large_message_fragments: vec![proto::LargeMessageFragment {
            message_id: Some(message_id.0.clone()),
            fragment_index: Some(0),
            total_fragments: Some(4),
            fragment_data: Some(vec![7u8; 128]),
            topic_id: Some(topic_hash.clone().into_string()),
        }],
    };

    let mut codec = GossipsubCodec::new(
        u32::MAX as usize,
        ValidationMode::Strict,
        HashMap::new(),
        5000,
        5000,
    );
    let mut buf = BytesMut::new();
    codec.encode(rpc, &mut buf).unwrap();
    let event = codec.decode(&mut buf).unwrap().unwrap();

    let HandlerEvent::Message {
        rpc,
        invalid_messages,
    } = event
    else {
        panic!("Expected message event");
    };
    assert!(invalid_messages.is_empty());
    assert!(
        rpc.control_msgs
            .contains(&ControlAction::Preamble(Preamble {
                message_id: message_id.clone(),
                message_size: 1 << 20,
                topic_hash: topic_hash.clone(),
            }))
    );
    assert!(
        rpc.control_msgs
            .contains(&ControlAction::ImReceiving(ImReceiving {
                message_id: message_id.clone(),
            }))
    );
    assert_eq!(
        rpc.large_message_fragments,
        vec![LargeMessageFragment {
            message_id,
            fragment_index: 0,
            total_fragments: 4,
            fragment_data: vec![7u8; 128],
            topic_hash,
        }]
    );

    // Delivering the decoded RPC to the behaviour is a no-op for now.
    let (mut gs, peers, _, _) = DefaultBehaviourTestBuilder::default()
        .peer_no(1)
        .create_network();
    gs.events.clear();
    gs.on_connection_handler_event(
        peers[0],
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc,
            invalid_messages: vec![],
        },
    );
    assert!(gs.events.is_empty());
}
