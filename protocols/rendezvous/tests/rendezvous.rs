pub mod harness;

use crate::harness::{await_events_or_timeout, connect, new_swarm};
use libp2p_core::identity::Keypair;
use libp2p_core::network::Peer;
use libp2p_core::{AuthenticatedPeerRecord, Multiaddr, PeerId};
use libp2p_rendezvous::behaviour::{Event, Rendezvous};
use libp2p_swarm::SwarmEvent;
use libp2p_swarm::{AddressScore, Swarm};
use std::str::FromStr;
use std::time::Duration;

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    env_logger::init();
    let mut test = RendezvousTest::setup().await;

    let namespace = "some-namespace".to_string();

    println!("registring");

    // register
    test.registration_swarm
        .behaviour_mut()
        .register(namespace.clone(), test.rendezvous_peer_id);

    test.assert_successful_registration(namespace.clone()).await;

    println!("Registration worked!");

    // // discover
    // test.discovery_swarm
    //     .behaviour_mut()
    //     .discover(Some(namespace), test.rendezvous_peer_id);
    //
    // test.assert_successful_discovery().await;
}

struct RendezvousTest {
    pub registration_swarm: Swarm<Rendezvous>,
    pub registration_peer_id: PeerId,

    pub discovery_swarm: Swarm<Rendezvous>,
    pub discovery_peer_id: PeerId,

    pub rendezvous_swarm: Swarm<Rendezvous>,
    pub rendezvous_peer_id: PeerId,
}

fn get_rand_listen_addr() -> Multiaddr {
    let address_port = rand::random::<u64>();
    let addr = format!("/memory/{}", address_port)
        .parse::<Multiaddr>()
        .unwrap();

    addr
}

impl RendezvousTest {
    pub async fn setup() -> Self {
        let registration_addr = get_rand_listen_addr();
        let discovery_addr = get_rand_listen_addr();
        let rendezvous_addr = get_rand_listen_addr();

        let (mut registration_swarm, _, registration_peer_id) = new_swarm(
            |_, identity| Rendezvous::new(identity, "Registration".to_string()),
            registration_addr.clone(),
        );
        registration_swarm.add_external_address(registration_addr, AddressScore::Infinite);

        let (mut discovery_swarm, _, discovery_peer_id) = new_swarm(
            |_, identity| Rendezvous::new(identity, "Discovery".to_string()),
            discovery_addr.clone(),
        );
        discovery_swarm.add_external_address(discovery_addr, AddressScore::Infinite);

        let (mut rendezvous_swarm, _, rendezvous_peer_id) = new_swarm(
            |_, identity| Rendezvous::new(identity, "Rendezvous".to_string()),
            rendezvous_addr.clone(),
        );
        rendezvous_swarm.add_external_address(rendezvous_addr, AddressScore::Infinite);

        //connect(&mut rendezvous_swarm, &mut discovery_swarm).await;
        connect(&mut rendezvous_swarm, &mut registration_swarm).await;

        Self {
            registration_swarm,
            registration_peer_id,
            discovery_swarm,
            discovery_peer_id,
            rendezvous_swarm,
            rendezvous_peer_id,
        }
    }

    pub async fn assert_successful_registration(&mut self, namespace: String) {
        let rendezvous_swarm = &mut self.rendezvous_swarm;
        let reggo_swarm = &mut self.registration_swarm;

        match await_events_or_timeout(async {
            let event = rendezvous_swarm.next_event().await;
            dbg!(event)
        }, async {
            let event = reggo_swarm.next_event().await;
            dbg!(event)
        }).await {
            (
                rendezvous_swarm_event,
                registration_swarm_event,
            ) => {

                // TODO: Assertion against the actual peer record, pass the peer record in for assertion

                assert!(matches!(rendezvous_swarm_event, SwarmEvent::Behaviour(Event::PeerRegistered { .. })));
                assert!(matches!(registration_swarm_event, SwarmEvent::Behaviour(Event::RegisteredWithRendezvousNode { .. })));
            }
            (rendezvous_swarm_event, registration_swarm_event) => panic!(
                "Received unexpected event, rendezvous swarm emitted {:?} and registration swarm emitted {:?}",
                rendezvous_swarm_event, registration_swarm_event
            ),
        }
    }

    pub async fn assert_successful_discovery(&mut self) {
        // TODO: Is it by design that there is no event emitted on the rendezvous side for discovery?

        match tokio::time::timeout(Duration::from_secs(10), self.discovery_swarm.next())
            .await
            .expect("")
        {
            // TODO: Assert against the actual registration
            Event::Discovered { .. } => {}
            event => panic!("Discovery swarm emitted unexpected event {:?}", event),
        }
    }
}
