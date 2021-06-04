use futures::{channel::mpsc, prelude::*};
use log::debug;
pub mod harness;
use crate::harness::{get_rand_listen_addr, mk_transport};
use futures::{SinkExt, StreamExt};
use libp2p_core::identity::Keypair;
use libp2p_core::Multiaddr;
use libp2p_rendezvous::behaviour::Rendezvous;
use libp2p_swarm::{Swarm, SwarmEvent};
use std::time::Duration;

#[test]
fn given_successful_registration_then_successful_discovery() {
    let _ = env_logger::builder().is_test(true).try_init().unwrap();

    let registration_keys = Keypair::generate_ed25519();
    let registration_peer_id = registration_keys.public().into_peer_id();

    let rendezvous_keys = Keypair::generate_ed25519();
    let rendezvoud_peer_id = rendezvous_keys.public().into_peer_id();
    let rendezvous_addr = get_rand_listen_addr();

    let mut registration_swarm = Swarm::new(
        mk_transport(registration_keys.clone()),
        Rendezvous::new(registration_keys, vec![]),
        registration_peer_id,
    );

    let mut rendezvous_swarm = Swarm::new(
        mk_transport(rendezvous_keys.clone()),
        Rendezvous::new(rendezvous_keys, vec![rendezvous_addr.clone()]),
        rendezvoud_peer_id,
    );

    let (mut tx, mut rx) = mpsc::channel::<Multiaddr>(1);

    rendezvous_swarm.listen_on(rendezvous_addr).unwrap();

    let rendezvous_future = async move {
        loop {
            let next = rendezvous_swarm.next_event().await;
            debug!("rendezvous swarm event: {:?}", next);
            match next {
                SwarmEvent::NewListenAddr(listener) => tx.send(listener).await.unwrap(),
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    debug!("connection closed: {:?}", peer_id);
                    return;
                }
                _ => {}
            }
        }
    };

    let registration_future = async move {
        registration_swarm
            .dial_addr(rx.next().await.unwrap())
            .unwrap();
        loop {
            let next = registration_swarm.next_event().await;
            debug!("registration swarm event: {:?}", next);
            match next {
                SwarmEvent::Behaviour(
                    libp2p_rendezvous::behaviour::Event::RegisteredWithRendezvousNode { .. },
                ) => {
                    debug!("registered with rendezvous node !!!!");
                    return;
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    registration_swarm
                        .behaviour_mut()
                        .register("xmr-btc".to_string(), peer_id);
                }
                _ => {}
            }
        }
    };

    let future = future::join(registration_future, rendezvous_future);

    let dur = Duration::from_secs(5);
    let timeout = async_std::future::timeout(dur, future);

    let (a, b) = async_std::task::block_on(timeout).unwrap();
}
