use futures::executor::LocalPool;
use futures::future::FutureExt;
use futures::stream::{Stream, StreamExt};
use futures::task::Spawn;
use libp2p::kad::record::store::MemoryStore;
use libp2p::NetworkBehaviour;
use libp2p_core::either::EitherTransport;
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::transport::{MemoryTransport, Transport, TransportError};
use libp2p_core::{identity, upgrade, PeerId};
use libp2p_identify::{Identify, IdentifyEvent, IdentifyInfo};
use libp2p_kad::{GetClosestPeersOk, Kademlia, KademliaEvent, QueryResult};
use libp2p_ping::{Ping, PingConfig, PingEvent};
use libp2p_plaintext::PlainText2Config;
use libp2p_relay::{Relay, RelayTransportWrapper};
use libp2p_swarm::{
    NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters, Swarm, SwarmEvent,
};
use std::task::{Context, Poll};
use std::time::Duration;

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "CombinedEvent", poll_method = "poll")]
struct CombinedBehaviour {
    relay: Relay,
    ping: Ping,
    kad: Kademlia<MemoryStore>,
    identify: Identify,

    #[behaviour(ignore)]
    events: Vec<CombinedEvent>,
}

#[derive(Debug)]
enum CombinedEvent {
    Kad(KademliaEvent),
    Ping(PingEvent),
}

impl CombinedBehaviour {
    fn poll<TEv>(
        &mut self,
        _: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<TEv, CombinedEvent>> {
        if !self.events.is_empty() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(self.events.remove(0)));
        }

        Poll::Pending
    }
}

impl NetworkBehaviourEventProcess<PingEvent> for CombinedBehaviour {
    fn inject_event(&mut self, event: PingEvent) {
        self.events.push(CombinedEvent::Ping(event));
    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for CombinedBehaviour {
    fn inject_event(&mut self, event: KademliaEvent) {
        self.events.push(CombinedEvent::Kad(event));
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for CombinedBehaviour {
    fn inject_event(&mut self, event: IdentifyEvent) {
        match event {
            IdentifyEvent::Received {
                peer_id,
                info: IdentifyInfo { listen_addrs, .. },
                ..
            } => {
                for addr in listen_addrs {
                    self.kad.add_address(&peer_id, addr);
                }
            }
            IdentifyEvent::Sent { .. } => {}
            e => panic!("{:?}", e),
        }
    }
}

impl NetworkBehaviourEventProcess<()> for CombinedBehaviour {
    fn inject_event(&mut self, _event: ()) {
        unreachable!();
    }
}

enum Reachability {
    Firewalled,
    Routable,
}

/// Wrapper around a [`Transport`] allowing outgoing but denying incoming connections.
///
/// Meant for testing purposes only.
#[derive(Clone)]
pub struct Firewall<T>(pub T);

impl<T: Transport> Transport for Firewall<T> {
    type Output = <T as Transport>::Output;
    type Error = <T as Transport>::Error;
    type Listener = futures::stream::Pending<<<T as Transport>::Listener as Stream>::Item>;
    type ListenerUpgrade = <T as Transport>::ListenerUpgrade;
    type Dial = <T as Transport>::Dial;

    fn listen_on(self, _: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        Ok(futures::stream::pending())
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.0.dial(addr)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.0.address_translation(server, observed)
    }
}

fn build_swarm(reachability: Reachability) -> Swarm<CombinedBehaviour> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let plaintext = PlainText2Config {
        local_public_key: local_public_key.clone(),
    };
    let local_peer_id = local_public_key.clone().into_peer_id();

    let transport = MemoryTransport::default();

    let transport = match reachability {
        Reachability::Firewalled => EitherTransport::Left(Firewall(transport)),
        Reachability::Routable => EitherTransport::Right(transport),
    };

    let (transport, (to_transport, from_transport)) = RelayTransportWrapper::new(transport);

    let transport = transport
        .upgrade(upgrade::Version::V1)
        .authenticate(plaintext)
        .multiplex(libp2p_yamux::YamuxConfig::default())
        .boxed();

    let combined_behaviour = CombinedBehaviour {
        relay: Relay::new(to_transport, from_transport),
        ping: Ping::new(PingConfig::new().with_interval(Duration::from_millis(10))),
        kad: Kademlia::new(
            local_peer_id.clone(),
            MemoryStore::new(local_peer_id.clone()),
        ),
        identify: Identify::new(
            "test".to_string(),
            "test-agent".to_string(),
            local_public_key.clone(),
        ),
        events: Default::default(),
    };

    Swarm::new(transport, combined_behaviour, local_peer_id)
}

#[test]
fn node_a_connect_to_node_b_listening_via_relay() {
    let _ = env_logger::try_init();

    let mut pool = LocalPool::new();

    let mut node_a_swarm = build_swarm(Reachability::Firewalled);
    let mut node_b_swarm = build_swarm(Reachability::Firewalled);
    let mut relay_swarm = build_swarm(Reachability::Routable);

    let node_a_peer_id = Swarm::local_peer_id(&node_a_swarm).clone();
    let node_b_peer_id = Swarm::local_peer_id(&node_b_swarm).clone();
    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();

    let relay_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(node_b_peer_id.into()));

    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            async move {
                loop {
                    relay_swarm.next_event().await;
                }
            }
            .boxed()
            .into(),
        )
        .unwrap();

    Swarm::listen_on(&mut node_b_swarm, node_b_address.clone()).unwrap();

    pool.run_until(async {
        // Node B reporting listen address via relay.
        assert!(matches!(
            node_b_swarm.next_event().await,
            SwarmEvent::NewListenAddr(_)
        ));

        // Node B dialing Relay.
        match node_b_swarm.next_event().await {
            SwarmEvent::Dialing(peer_id) => assert_eq!(peer_id, relay_peer_id),
            e => panic!("{:?}", e),
        }

        // Node B establishing connection to Relay.
        match node_b_swarm.next_event().await {
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                assert_eq!(peer_id, relay_peer_id);
            }
            e => panic!("{:?}", e),
        }

        let node_b = async move {
            // Node B receiving connection from Node A via Relay.
            loop {
                match node_b_swarm.next_event().await {
                    SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                        assert_eq!(
                            send_back_addr,
                            Protocol::P2p(node_a_peer_id.clone().into()).into()
                        );
                        break;
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node B establishing connection from Node A via Relay.
            loop {
                match node_b_swarm.next_event().await {
                    SwarmEvent::ConnectionEstablished { peer_id, .. }
                        if peer_id == node_a_peer_id =>
                    {
                        break;
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node B waiting for Ping from Node A via Relay.
            loop {
                match node_b_swarm.next_event().await {
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        assert_eq!(peer_id, node_a_peer_id);
                        break;
                    }
                    e => panic!("{:?}", e),
                }
            }
        };

        Swarm::dial_addr(&mut node_a_swarm, node_b_address).unwrap();
        let node_a = async move {
            // Node A dialing Relay to connect to Node B.
            loop {
                match node_a_swarm.next_event().await {
                    SwarmEvent::Dialing(peer_id) => {
                        assert_eq!(peer_id, relay_peer_id);
                        break;
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node A establishing connection to Relay to connect to Node B.
            loop {
                match node_a_swarm.next_event().await {
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        assert_eq!(peer_id, relay_peer_id);
                        break;
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node A establishing connection to node B via Relay.
            loop {
                match node_a_swarm.next_event().await {
                    SwarmEvent::ConnectionEstablished { peer_id, .. }
                        if peer_id == node_b_peer_id =>
                    {
                        break
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node A waiting for Ping from Node B via Relay.
            loop {
                match node_a_swarm.next_event().await {
                    SwarmEvent::Behaviour(CombinedEvent::Ping(PingEvent {
                        peer,
                        result: Ok(_),
                    })) => {
                        if peer == node_b_peer_id {
                            break;
                        }
                    }
                    e => panic!("{:?}", e),
                }
            }
        };

        futures::future::join(node_b, node_a).await
    });
}

#[test]
fn node_a_connect_to_node_b_not_listening_via_relay() {
    let _ = env_logger::try_init();

    let mut pool = LocalPool::new();

    let mut node_a_swarm = build_swarm(Reachability::Firewalled);
    let mut node_b_swarm = build_swarm(Reachability::Routable);
    let mut relay_swarm = build_swarm(Reachability::Routable);

    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();
    let node_b_peer_id = Swarm::local_peer_id(&node_b_swarm).clone();

    let relay_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address_via_relay = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.clone().into()))
        .with(Protocol::P2pCircuit)
        .with(node_b_address.into_iter().next().unwrap())
        .with(Protocol::P2p(node_b_peer_id.clone().into()));

    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            StreamExt::collect::<Vec<_>>(relay_swarm)
                .map(|_| ())
                .boxed()
                .into(),
        )
        .unwrap();

    Swarm::listen_on(&mut node_b_swarm, node_b_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            StreamExt::collect::<Vec<_>>(node_b_swarm)
                .map(|_| ())
                .boxed()
                .into(),
        )
        .unwrap();

    Swarm::dial_addr(&mut node_a_swarm, node_b_address_via_relay).unwrap();
    pool.run_until(async move {
        // Node A dialing Relay to connect to Node B.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::Dialing(peer_id) => {
                    assert_eq!(peer_id, relay_peer_id);
                    break;
                }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        // Node A establishing connection to Relay to connect to Node B.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    assert_eq!(peer_id, relay_peer_id);
                    break;
                }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        // Node A establishing connection to node B via Relay.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == node_b_peer_id => {
                    break
                }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        // Node A waiting for Ping from Node B via Relay.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::Behaviour(CombinedEvent::Ping(PingEvent {
                    peer,
                    result: Ok(_),
                })) => {
                    if peer == node_b_peer_id {
                        break;
                    }
                }
                e => panic!("{:?}", e),
            }
        }
    });
}

#[test]
fn node_a_try_connect_to_offline_node_b() {
    let _ = env_logger::try_init();

    let mut pool = LocalPool::new();

    let mut node_a_swarm = build_swarm(Reachability::Firewalled);
    let mut relay_swarm = build_swarm(Reachability::Routable);

    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();
    let node_b_peer_id = PeerId::random();

    let relay_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address_via_relay = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.clone().into()))
        .with(Protocol::P2pCircuit)
        .with(node_b_address.into_iter().next().unwrap())
        .with(Protocol::P2p(node_b_peer_id.clone().into()));

    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            StreamExt::collect::<Vec<_>>(relay_swarm)
                .map(|_| ())
                .boxed()
                .into(),
        )
        .unwrap();

    Swarm::dial_addr(&mut node_a_swarm, node_b_address_via_relay.clone()).unwrap();
    pool.run_until(async move {
        // Node A dialing Relay to connect to Node B.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::Dialing(peer_id) => {
                    assert_eq!(peer_id, relay_peer_id);
                    break;
                }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        // Node A establishing connection to Relay to connect to Node B.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    assert_eq!(peer_id, relay_peer_id);
                    break;
                }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::UnknownPeerUnreachableAddr { address, .. }
                    if address == node_b_address_via_relay =>
                {
                    break;
                }
                // SwarmEvent::UnreachableAddr { peer_id, .. } => {
                //     if peer_id == node_b_peer_id {
                //         break;
                //     }
                // }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }
    });
}

#[test]
fn node_a_try_connect_to_offline_node_b_via_offline_relay() {
    let _ = env_logger::try_init();

    let mut pool = LocalPool::new();

    let mut node_a_swarm = build_swarm(Reachability::Firewalled);

    let relay_peer_id = PeerId::random();
    let node_b_peer_id = PeerId::random();

    let relay_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address_via_relay = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.clone().into()))
        .with(Protocol::P2pCircuit)
        .with(node_b_address.into_iter().next().unwrap())
        .with(Protocol::P2p(node_b_peer_id.clone().into()));

    Swarm::dial_addr(&mut node_a_swarm, node_b_address_via_relay.clone()).unwrap();
    pool.run_until(async move {
        // Node A dialing Relay to connect to Node B.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::Dialing(peer_id) => {
                    assert_eq!(peer_id, relay_peer_id);
                    break;
                }
                e => panic!("{:?}", e),
            }
        }

        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::UnreachableAddr { peer_id, .. } if peer_id == relay_peer_id => {
                    break;
                }
                SwarmEvent::UnknownPeerUnreachableAddr { address, .. }
                    if address == node_b_address_via_relay =>
                {
                    break;
                }
                e => panic!("{:?}", e),
            }
        }
    });
}

#[test]
fn node_a_connect_to_node_b_via_established_connection_to_relay() {
    let _ = env_logger::try_init();

    let mut pool = LocalPool::new();

    let mut node_a_swarm = build_swarm(Reachability::Firewalled);
    let mut node_b_swarm = build_swarm(Reachability::Routable);
    let mut relay_swarm = build_swarm(Reachability::Routable);

    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();
    let node_b_peer_id = Swarm::local_peer_id(&node_b_swarm).clone();

    let relay_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address_via_relay = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.clone().into()))
        .with(Protocol::P2pCircuit)
        .with(node_b_address.into_iter().next().unwrap())
        .with(Protocol::P2p(node_b_peer_id.clone().into()));

    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            StreamExt::collect::<Vec<_>>(relay_swarm)
                .map(|_| ())
                .boxed()
                .into(),
        )
        .unwrap();

    Swarm::listen_on(&mut node_b_swarm, node_b_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            StreamExt::collect::<Vec<_>>(node_b_swarm)
                .map(|_| ())
                .boxed()
                .into(),
        )
        .unwrap();

    pool.run_until(async move {
        Swarm::dial_addr(&mut node_a_swarm, relay_address).unwrap();
        // Node A dialing Relay.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::Dialing(peer_id) => {
                    assert_eq!(peer_id, relay_peer_id);
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    assert_eq!(peer_id, relay_peer_id);
                    break;
                }
                e => panic!("{:?}", e),
            }
        }

        Swarm::dial_addr(&mut node_a_swarm, node_b_address_via_relay).unwrap();

        // Node A establishing connection to node B via Relay.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == node_b_peer_id => {
                    break
                }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        // Node A waiting for Ping from Node B via Relay.
        loop {
            match node_a_swarm.next_event().await {
                SwarmEvent::Behaviour(CombinedEvent::Ping(PingEvent {
                    peer,
                    result: Ok(_),
                })) => {
                    if peer == node_b_peer_id {
                        break;
                    }
                }
                e => panic!("{:?}", e),
            }
        }
    });
}

#[test]
fn firewalled_node_a_discover_firewalled_node_b_via_kad_and_connect_to_node_b_via_routable_relay() {
    let _ = env_logger::try_init();

    let mut pool = LocalPool::new();

    let mut node_a_swarm = build_swarm(Reachability::Firewalled);
    let mut node_b_swarm = build_swarm(Reachability::Firewalled);
    let mut relay_swarm = build_swarm(Reachability::Routable);

    let node_a_peer_id = Swarm::local_peer_id(&node_a_swarm).clone();
    let node_b_peer_id = Swarm::local_peer_id(&node_b_swarm).clone();
    let relay_peer_id = Swarm::local_peer_id(&relay_swarm).clone();

    let relay_address: Multiaddr = Protocol::Memory(rand::random::<u64>()).into();
    let node_b_address = relay_address
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(node_b_peer_id.into()));

    node_a_swarm
        .kad
        .add_address(&relay_peer_id, relay_address.clone());
    node_b_swarm
        .kad
        .add_address(&relay_peer_id, relay_address.clone());

    Swarm::listen_on(&mut relay_swarm, relay_address.clone()).unwrap();
    pool.spawner()
        .spawn_obj(
            async move {
                loop {
                    match relay_swarm.next_event().await {
                        SwarmEvent::Behaviour(CombinedEvent::Kad(_)) => {}
                        SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                        _ => (),
                    }
                }
            }
            .boxed()
            .into(),
        )
        .unwrap();

    // Node B listen via Relay.
    Swarm::listen_on(&mut node_b_swarm, node_b_address.clone()).unwrap();

    pool.run_until(async {
        // Node B reporting listen address via relay.
        assert!(matches!(
            node_b_swarm.next_event().await,
            SwarmEvent::NewListenAddr(_)
        ));

        // Node B dialing Relay.
        match node_b_swarm.next_event().await {
            SwarmEvent::Dialing(peer_id) => assert_eq!(peer_id, relay_peer_id),
            e => panic!("{:?}", e),
        }

        // Node B establishing connection to Relay.
        loop {
            match node_b_swarm.next_event().await {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    assert_eq!(peer_id, relay_peer_id);
                    break;
                }
                SwarmEvent::Behaviour(CombinedEvent::Kad(_)) => {}
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        // Node B bootstrapping.
        let query_id = node_b_swarm.kad.bootstrap().unwrap();
        loop {
            match node_b_swarm.next_event().await {
                SwarmEvent::Behaviour(CombinedEvent::Kad(KademliaEvent::QueryResult {
                    id,
                    result: QueryResult::Bootstrap(Ok(_)),
                    ..
                })) if query_id == id => {
                    if node_b_swarm.kad.iter_queries().count() == 0 {
                        break;
                    }
                }
                SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                e => panic!("{:?}", e),
            }
        }

        let node_b = async move {
            // Node B receiving connection from Node A via Relay.
            loop {
                match node_b_swarm.next_event().await {
                    SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                        assert_eq!(send_back_addr, Protocol::P2p(node_a_peer_id.into()).into());
                        break;
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node B establishing connection from Node A via Relay.
            loop {
                match node_b_swarm.next_event().await {
                    SwarmEvent::ConnectionEstablished { peer_id, .. }
                        if peer_id == node_a_peer_id =>
                    {
                        break;
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node B waiting for Node A to close connection.
            loop {
                match node_b_swarm.next_event().await {
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        assert_eq!(peer_id, node_a_peer_id);
                        break;
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Kad(_)) => {}
                    e => panic!("{:?}", e),
                }
            }
        };

        let node_a = async move {
            // Node A looking for Node B on the Kademlia DHT.
            let mut query_id = node_a_swarm.kad.get_closest_peers(node_b_peer_id);
            // One has to retry multiple times to wait for Relay to receive Identify event from Node
            // B.
            let mut tries = 0;

            // Node A establishing connection to Relay and, given that the DHT is small, Node
            // B.
            loop {
                match node_a_swarm.next_event().await {
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        if peer_id == relay_peer_id {
                            continue;
                        } else if peer_id == node_b_peer_id {
                            break;
                        } else {
                            panic!("Unexpected peer id {:?}", peer_id);
                        }
                    }
                    SwarmEvent::Dialing(peer_id)
                        if peer_id == relay_peer_id || peer_id == node_b_peer_id => {}
                    SwarmEvent::Behaviour(CombinedEvent::Ping(_)) => {}
                    SwarmEvent::Behaviour(CombinedEvent::Kad(KademliaEvent::QueryResult {
                        id,
                        result: QueryResult::GetClosestPeers(Ok(GetClosestPeersOk { .. })),
                        ..
                    })) if id == query_id => {
                        tries += 1;
                        if tries > 300 {
                            panic!("Too many retries.");
                        }

                        query_id = node_a_swarm.kad.get_closest_peers(node_b_peer_id);
                    }
                    SwarmEvent::Behaviour(CombinedEvent::Kad(KademliaEvent::RoutingUpdated {
                        ..
                    })) => {}
                    e => panic!("{:?}", e),
                }
            }

            // Node A waiting for Ping from Node B via Relay.
            loop {
                match node_a_swarm.next_event().await {
                    SwarmEvent::Behaviour(CombinedEvent::Ping(PingEvent {
                        peer,
                        result: Ok(_),
                    })) => {
                        if peer == node_b_peer_id {
                            break;
                        }
                    }
                    e => panic!("{:?}", e),
                }
            }
        };

        futures::future::join(node_b, node_a).await
    });
}
