pub mod harness;

use crate::harness::{await_events_or_timeout, new_swarm, SwarmExt};
use libp2p_core::PeerId;
use libp2p_rendezvous::behaviour::{Event, RegisterError, Rendezvous};
use libp2p_rendezvous::codec::{ErrorCode, DEFAULT_TTL};
use libp2p_swarm::{Swarm, SwarmEvent};

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    let _ = env_logger::try_init();
    let mut test = RendezvousTest::setup().await;

    let namespace = "some-namespace".to_string();

    let _ = test.registration_swarm.behaviour_mut().register(
        namespace.clone(),
        *test.rendezvous_swarm.local_peer_id(),
        None,
    );

    test.assert_successful_registration(namespace.clone(), DEFAULT_TTL)
        .await;

    test.discovery_swarm.behaviour_mut().discover(
        Some(namespace.clone()),
        None,
        *test.rendezvous_swarm.local_peer_id(),
    );

    test.assert_successful_discovery(
        namespace.clone(),
        DEFAULT_TTL,
        *test.registration_swarm.local_peer_id(),
    )
    .await;
}

#[tokio::test]
async fn given_successful_registration_then_refresh_ttl() {
    let _ = env_logger::try_init();
    let mut test = RendezvousTest::setup().await;

    let namespace = "some-namespace".to_string();

    let refesh_ttl = 10_000;

    let _ = test.registration_swarm.behaviour_mut().register(
        namespace.clone(),
        *test.rendezvous_swarm.local_peer_id(),
        None,
    );

    test.assert_successful_registration(namespace.clone(), DEFAULT_TTL)
        .await;

    test.discovery_swarm.behaviour_mut().discover(
        Some(namespace.clone()),
        None,
        *test.rendezvous_swarm.local_peer_id(),
    );

    test.assert_successful_discovery(
        namespace.clone(),
        DEFAULT_TTL,
        *test.registration_swarm.local_peer_id(),
    )
    .await;

    let _ = test.registration_swarm.behaviour_mut().register(
        namespace.clone(),
        *test.rendezvous_swarm.local_peer_id(),
        Some(refesh_ttl),
    );

    test.assert_successful_registration(namespace.clone(), refesh_ttl)
        .await;

    test.discovery_swarm.behaviour_mut().discover(
        Some(namespace.clone()),
        None,
        *test.rendezvous_swarm.local_peer_id(),
    );

    test.assert_successful_discovery(
        namespace.clone(),
        refesh_ttl,
        *test.registration_swarm.local_peer_id(),
    )
    .await;
}

#[tokio::test]
async fn given_invalid_ttl_then_unsuccessful_registration() {
    let _ = env_logger::try_init();
    let mut test = RendezvousTest::setup().await;

    let namespace = "some-namespace".to_string();

    let _ = test.registration_swarm.behaviour_mut().register(
        namespace.clone(),
        *test.rendezvous_swarm.local_peer_id(),
        Some(100_000),
    );

    match await_events_or_timeout(&mut test.rendezvous_swarm, &mut test.registration_swarm).await {
        (
            SwarmEvent::Behaviour(Event::PeerNotRegistered { .. }),
            SwarmEvent::Behaviour(Event::RegisterFailed(RegisterError::Remote { error: err_code , ..})),
        ) => {
            assert_eq!(err_code, ErrorCode::InvalidTtl);
        }
        (rendezvous_swarm_event, registration_swarm_event) => panic!(
            "Received unexpected event, rendezvous swarm emitted {:?} and registration swarm emitted {:?}",
            rendezvous_swarm_event, registration_swarm_event
        ),
    }
}

struct RendezvousTest {
    pub registration_swarm: Swarm<Rendezvous>,
    pub discovery_swarm: Swarm<Rendezvous>,
    pub rendezvous_swarm: Swarm<Rendezvous>,
}

const DEFAULT_TTL_UPPER_BOUND: i64 = 56_000;

impl RendezvousTest {
    pub async fn setup() -> Self {
        let mut registration_swarm =
            new_swarm(|_, identity| Rendezvous::new(identity, DEFAULT_TTL_UPPER_BOUND));
        registration_swarm.listen_on_random_memory_address().await;

        let mut discovery_swarm =
            new_swarm(|_, identity| Rendezvous::new(identity, DEFAULT_TTL_UPPER_BOUND));
        discovery_swarm.listen_on_random_memory_address().await;

        let mut rendezvous_swarm =
            new_swarm(|_, identity| Rendezvous::new(identity, DEFAULT_TTL_UPPER_BOUND));
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

    pub async fn assert_successful_registration(
        &mut self,
        expected_namespace: String,
        expected_ttl: i64,
    ) {
        match await_events_or_timeout(&mut self.rendezvous_swarm, &mut self.registration_swarm).await {
            (
                SwarmEvent::Behaviour(Event::PeerRegistered { peer, registration }),
                SwarmEvent::Behaviour(Event::Registered { rendezvous_node, ttl, namespace: register_node_namespace }),
            ) => {
                assert_eq!(&peer, self.registration_swarm.local_peer_id());
                assert_eq!(&rendezvous_node, self.rendezvous_swarm.local_peer_id());
                assert_eq!(registration.namespace, expected_namespace);
                assert_eq!(register_node_namespace, expected_namespace);
                assert_eq!(ttl, expected_ttl);
            }
            (rendezvous_swarm_event, registration_swarm_event) => panic!(
                "Received unexpected event, rendezvous swarm emitted {:?} and registration swarm emitted {:?}",
                rendezvous_swarm_event, registration_swarm_event
            ),
        }
    }

    pub async fn assert_successful_discovery(
        &mut self,
        expected_namespace: String,
        expected_ttl: i64,
        expected_peer_id: PeerId,
    ) {
        match await_events_or_timeout(&mut self.rendezvous_swarm, &mut self.discovery_swarm).await {
            (
                SwarmEvent::Behaviour(Event::DiscoverServed { .. }),
                SwarmEvent::Behaviour(Event::Discovered { registrations, .. }),
            ) => {
                if let Some(reg) =
                    registrations.get(&(expected_namespace.clone(), expected_peer_id))
                {
                    assert_eq!(reg.ttl, expected_ttl)
                } else {
                    {
                        panic!(
                            "Registration with namespace {} and peer id {} not found",
                            expected_namespace, expected_peer_id
                        )
                    }
                }
            }
            (e1, e2) => panic!("Unexpected events {:?} {:?}", e1, e2),
        }
    }
}
