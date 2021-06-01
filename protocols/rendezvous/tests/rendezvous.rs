pub mod harness;
use crate::harness::{await_events_or_timeout, connect, new_swarm};
use libp2p_core::AuthenticatedPeerRecord;
use libp2p_rendezvous::behaviour::{Event, Rendezvous};
use libp2p_swarm::Swarm;
use std::time::Duration;

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    let mut test = RendezvousTest::setup().await;

    let namespace = "some-namespace".to_string();

    // register
    test.registration_swarm.behaviour_mut().register(
        namespace.clone(),
        test.rendezvous_peer_id,
        todo!(),
    );

    test.assert_successful_registration(namespace.clone()).await;

    // discover
    test.discovery_swarm
        .behaviour_mut()
        .discover(Some(namespace), test.rendezvous_peer_id);

    test.assert_successful_discovery().await;
}

struct RendezvousTest {
    pub registration_swarm: Swarm<Rendezvous>,
    pub registration_peer_id: AuthenticatedPeerRecord,

    pub discovery_swarm: Swarm<Rendezvous>,
    pub discovery_peer_id: AuthenticatedPeerRecord,

    pub rendezvous_swarm: Swarm<Rendezvous>,
    pub rendezvous_peer_id: AuthenticatedPeerRecord,
}

impl RendezvousTest {
    pub async fn setup() -> Self {
        let (mut registration_swarm, _, registration_peer_id) =
            harness::new_swarm(|_, _| Rendezvous::new());
        let (mut discovery_swarm, _, discovery_peer_id) = new_swarm(|_, _| Rendezvous::new());

        let (mut rendezvous_swarm, _, rendezvous_peer_id) = new_swarm(|_, _| Rendezvous::new());

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

                // TODO: Assertion against the actual peer record possible?
                // let expected = Event::PeerRegistered { peer_id: self.registration_peer_id, ns: Registration { namespace, record: AuthenticatedPeerRecord::from_record() } };


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
