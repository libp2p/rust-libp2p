pub mod harness;
use crate::harness::{await_events_or_timeout, connect, new_swarm};
use libp2p_core::{AuthenticatedPeerRecord, PeerId, Multiaddr};
use libp2p_rendezvous::behaviour::{Event, Rendezvous};
use libp2p_swarm::Swarm;
use std::time::Duration;
use libp2p_core::network::Peer;
use libp2p_core::identity::Keypair;
use std::str::FromStr;

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    let mut test = RendezvousTest::setup().await;

    let namespace = "some-namespace".to_string();

    println!("registring");

    // register
    test.registration_swarm.behaviour_mut().register(
        namespace.clone(),
        test.rendezvous_peer_id,
    );

    test.assert_successful_registration(namespace.clone()).await;

    println!("Registration worked!");

    // discover
    test.discovery_swarm
        .behaviour_mut()
        .discover(Some(namespace), test.rendezvous_peer_id);

    test.assert_successful_discovery().await;
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

        let registration_keys = Keypair::generate_ed25519();
        let discovery_keys = Keypair::generate_ed25519();
        let rendezvous_keys = Keypair::generate_ed25519();

        let registration_addr = get_rand_listen_addr();
        let discovery_addr = get_rand_listen_addr();
        let rendezvous_addr = get_rand_listen_addr();

        let (mut registration_swarm, _, registration_peer_id) =
            new_swarm(|_, _| Rendezvous::new(
                registration_keys.clone(),
                vec![registration_addr.clone()]),
                               registration_keys.clone(),
                               registration_addr.clone()
            );

        let (mut discovery_swarm, _, discovery_peer_id) =
            new_swarm(|_, _| Rendezvous::new(
            discovery_keys.clone(),
            vec![discovery_addr.clone()]),
                      discovery_keys.clone(),
                      discovery_addr.clone()
            );

        let (mut rendezvous_swarm, _, rendezvous_peer_id) =
            new_swarm(|_, _| Rendezvous::new(
                rendezvous_keys.clone(),
                vec![rendezvous_addr.clone()]),
                      rendezvous_keys.clone(),
                rendezvous_addr.clone(),
            );

        connect(&mut rendezvous_swarm, &mut discovery_swarm).await;
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
        match await_events_or_timeout(self.rendezvous_swarm.next(), self.registration_swarm.next()).await {
            (
                rendezvous_swarm_event,
                registration_swarm_event,
            ) => {

                // TODO: Assertion against the actual peer record, pass the peer record in for assertion

                assert!(matches!(rendezvous_swarm_event, Event::PeerRegistered { .. }));
                assert!(matches!(registration_swarm_event, Event::RegisteredWithRendezvousNode { .. }));
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
