use std::collections::BTreeSet;

use hashlink::LinkedHashMap;
use libp2p_core::PeerId;
use libp2p_swarm::{ConnectionId, NetworkBehaviour};

use crate::{
    handler::HandlerEvent,
    queue::Queue,
    rpc_proto::proto,
    types::{ControlAction, Extensions, PeerDetails, PeerKind, RpcIn, RpcOut},
    Behaviour, ConfigBuilder, MessageAuthenticity, ValidationMode,
};

#[test]
fn test_extensions_message_creation() {
    let extensions_rpc = RpcOut::Extensions(Extensions {
        test_extension: Some(true),
        partial_messages: None,
    });
    let proto_rpc: proto::RPC = extensions_rpc.into();

    assert!(proto_rpc.control.is_some());
    let control = proto_rpc.control.unwrap();
    assert!(control.extensions.is_some());
    let test_extension = control.extensions.unwrap().testExtension.unwrap();
    assert!(test_extension);
    assert!(control.ihave.is_empty());
    assert!(control.iwant.is_empty());
    assert!(control.graft.is_empty());
    assert!(control.prune.is_empty());
    assert!(control.idontwant.is_empty());
}

#[test]
fn test_handle_extensions_message() {
    let mut gs: Behaviour = Behaviour::new(
        MessageAuthenticity::Anonymous,
        ConfigBuilder::default()
            .validation_mode(ValidationMode::None)
            .build()
            .unwrap(),
    )
    .unwrap();

    let peer_id = PeerId::random();
    let messages = Queue::new(gs.config.connection_handler_queue_len());

    // Add peer without extensions
    gs.connected_peers.insert(
        peer_id,
        PeerDetails {
            kind: PeerKind::Gossipsubv1_3,
            connections: vec![ConnectionId::new_unchecked(0)],
            outbound: false,
            topics: BTreeSet::new(),
            messages,
            dont_send: LinkedHashMap::new(),
            extensions: None,
            #[cfg(feature = "partial_messages")]
            partial_messages: Default::default(),
            #[cfg(feature = "partial_messages")]
            partial_opts: Default::default(),
        },
    );

    // Simulate receiving extensions message
    let extensions = Extensions {
        test_extension: Some(false),
        partial_messages: None,
    };
    gs.handle_extensions(&peer_id, extensions);

    // Verify extensions were stored
    let peer_details = gs.connected_peers.get(&peer_id).unwrap();
    assert!(peer_details.extensions.is_some());

    // Simulate receiving duplicate extensions message from another peer
    let duplicate_rpc = RpcIn {
        messages: vec![],
        subscriptions: vec![],
        control_msgs: vec![ControlAction::Extensions(Some(Extensions {
            test_extension: Some(true),
            partial_messages: None,
        }))],
        test_extension: None,
        #[cfg(feature = "partial_messages")]
        partial_message: None,
    };

    gs.on_connection_handler_event(
        peer_id,
        ConnectionId::new_unchecked(0),
        HandlerEvent::Message {
            rpc: duplicate_rpc,
            invalid_messages: vec![],
        },
    );

    // Extensions should still be present (not cleared or changed)
    let peer_details = gs.connected_peers.get(&peer_id).unwrap();
    let test_extension = peer_details.extensions.unwrap().test_extension.unwrap();
    assert!(!test_extension);
}
