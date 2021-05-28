pub mod harness;
use libp2p_swarm::Swarm;
use libp2p_core::PeerId;
use crate::harness::{new_swarm, connect};
use libp2p_rendezvous::behaviour::Rendezvous;

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    let mut test = RendezvousTest::setup().await;

}

struct RendezvousTest {
    alice_swarm: Swarm<Rendezvous>,
    bob_swarm: Swarm<Rendezvous>,

    alice_peer_id: PeerId,
}

impl RendezvousTest {
    pub async fn setup() -> Self {
        let (mut alice_swarm, _, alice_peer_id) = harness::new_swarm(|_, _| {
            Rendezvous::new()
        });
        let (mut bob_swarm, ..) = new_swarm(|_, _| Rendezvous::new() );

        connect(&mut alice_swarm, &mut bob_swarm).await;

        Self {
            alice_swarm,
            bob_swarm,
            alice_peer_id,
        }
    }

    pub async fn alice_registers_with_bob(&self, namespace: String) {
    }
}
