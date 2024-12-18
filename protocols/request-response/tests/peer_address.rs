use std::iter;

use libp2p_core::ConnectedPoint;
use libp2p_request_response as request_response;
use libp2p_request_response::ProtocolSupport;
use libp2p_swarm::{StreamProtocol, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use serde::{Deserialize, Serialize};

#[async_std::test]
#[cfg(feature = "cbor")]
async fn dial_succeeds_after_adding_peers_address() {
    libp2p_test_utils::with_default_env_filter();

    let protocols = iter::once((StreamProtocol::new("/ping/1"), ProtocolSupport::Full));
    let config = request_response::Config::default();

    let mut swarm = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols.clone(), config.clone())
    });

    let mut swarm2 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols.clone(), config.clone())
    });

    let peer_id2 = *swarm2.local_peer_id();

    let (listen_addr, _) = swarm2.listen().with_memory_addr_external().await;

    swarm.add_peer_address(peer_id2, listen_addr.clone());

    swarm.dial(peer_id2).unwrap();

    async_std::task::spawn(swarm2.loop_on_next());

    let (connected_peer_id, connected_address) = swarm
        .wait(|event| match event {
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                let address = match endpoint {
                    ConnectedPoint::Dialer { address, .. } => Some(address),
                    _ => None,
                };
                Some((peer_id, address))
            }
            _ => None,
        })
        .await;
    let expected_address = listen_addr.with_p2p(peer_id2).unwrap();

    assert_eq!(connected_peer_id, peer_id2);
    assert_eq!(expected_address, connected_address.unwrap());
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Ping(Vec<u8>);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pong(Vec<u8>);
