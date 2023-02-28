use libp2p_autonat::Behaviour;
use libp2p_core::Multiaddr;
use libp2p_swarm::{AddressScore, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt as _;

// autonat only works with TCP, so we can't use `listen` here.
pub async fn listen_on_random_tcp_address(swarm: &mut Swarm<Behaviour>) -> Multiaddr {
    let tcp_addr_listener_id = swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();

    // block until we are actually listening
    let multiaddr = swarm
        .wait(|e| match e {
            SwarmEvent::NewListenAddr {
                address,
                listener_id,
            } => (listener_id == tcp_addr_listener_id).then_some(address),
            other => {
                log::debug!(
                    "Ignoring {:?} while waiting for listening to succeed",
                    other
                );
                None
            }
        })
        .await;

    swarm.add_external_address(multiaddr.clone(), AddressScore::Infinite);

    multiaddr
}
