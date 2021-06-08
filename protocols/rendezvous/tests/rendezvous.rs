pub mod harness;

use crate::harness::{await_events_or_timeout, new_swarm, SwarmExt};
use libp2p_rendezvous::behaviour::{Event, Rendezvous};
use libp2p_swarm::Swarm;

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    env_logger::init();
    let mut test = RendezvousTest::setup().await;

    let namespace = "some-namespace".to_string();

    test.registration_swarm.behaviour_mut().register(
        namespace.clone(),
        test.rendezvous_swarm.local_peer_id().clone(),
    );
    test.assert_successful_registration(namespace.clone()).await;

    test.discovery_swarm.behaviour_mut().discover(
        Some(namespace),
        test.rendezvous_swarm.local_peer_id().clone(),
    );

    test.assert_successful_discovery().await;
}

struct RendezvousTest {
    pub registration_swarm: Swarm<Rendezvous>,
    pub discovery_swarm: Swarm<Rendezvous>,
    pub rendezvous_swarm: Swarm<Rendezvous>,
}

impl RendezvousTest {
    pub async fn setup() -> Self {
        let mut registration_swarm = new_swarm(|_, identity| Rendezvous::new(identity));
        registration_swarm.listen_on_random_memory_address().await;

        let mut discovery_swarm = new_swarm(|_, identity| Rendezvous::new(identity));
        discovery_swarm.listen_on_random_memory_address().await;

        let mut rendezvous_swarm = new_swarm(|_, identity| Rendezvous::new(identity));
        rendezvous_swarm.listen_on_random_memory_address().await;

        registration_swarm
            .block_on_connection(&mut rendezvous_swarm)
            .await;
        discovery_swarm
            .block_on_connection(&mut rendezvous_swarm)
            .await;

        Self {
            registration_swarm,
            discovery_swarm,
            rendezvous_swarm,
        }
    }

    pub async fn assert_successful_registration(&mut self, expected_namespace: String) {
        match await_events_or_timeout(self.rendezvous_swarm.next(), self.registration_swarm.next()).await {
            (
                Event::PeerRegistered { peer, namespace },
                Event::RegisteredWithRendezvousNode { rendezvous_node, .. },
            ) => {
                assert_eq!(&peer, self.registration_swarm.local_peer_id());
                assert_eq!(&rendezvous_node, self.rendezvous_swarm.local_peer_id());
                assert_eq!(namespace, expected_namespace);
            }
            (rendezvous_swarm_event, registration_swarm_event) => panic!(
                "Received unexpected event, rendezvous swarm emitted {:?} and registration swarm emitted {:?}",
                rendezvous_swarm_event, registration_swarm_event
            ),
        }
    }

    pub async fn assert_successful_discovery(&mut self) {
        match await_events_or_timeout(self.rendezvous_swarm.next(), self.discovery_swarm.next())
            .await
        {
            (Event::AnsweredDiscoverRequest { .. }, Event::Discovered { .. }) => {}
            (e1, e2) => panic!("Unexpected events {:?} {:?}", e1, e2),
        }
    }
}
