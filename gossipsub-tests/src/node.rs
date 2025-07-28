use crate::redis::RedisClient;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::futures::{FutureExt, StreamExt};
use libp2p::gossipsub::{
    AllowAllSubscriptionFilter, Behaviour, Config, Event, IdentityTransform, MessageAuthenticity,
};
use libp2p::identity::{Keypair, PeerId};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, Swarm, Transport};
use std::time::Duration;
use tracing::{debug, info};

/// Creates a gossipsub behavior for testing.
pub(crate) fn gossipsub(keypair: Keypair) -> Result<Behaviour, &'static str> {
    Behaviour::new_with_subscription_filter_and_transform(
        MessageAuthenticity::Signed(keypair),
        Config::default(),
        AllowAllSubscriptionFilter {},
        IdentityTransform {},
    )
}

/// Creates a configured libp2p transport stack for gossipsub testing.
pub(crate) fn transport(
    keypair: &Keypair,
) -> libp2p::core::transport::Boxed<(PeerId, StreamMuxerBox)> {
    let tcp = libp2p::tcp::tokio::Transport::new(libp2p::tcp::Config::default().nodelay(true));
    let transport = libp2p::dns::tokio::Transport::system(tcp)
        .expect("DNS")
        .boxed();

    transport
        .upgrade(libp2p::core::upgrade::Version::V1)
        .authenticate(
            libp2p::noise::Config::new(keypair)
                .expect("signing can fail only once during starting a node"),
        )
        .multiplex(libp2p::yamux::Config::default())
        .timeout(Duration::from_secs(20))
        .boxed()
}

/// Synchronizes test nodes and drives the swarm until all nodes reach a barrier.
///
/// This function processes swarm events while waiting for all test nodes to signal
/// completion of a specific test phase using Redis coordination.
///
/// # Arguments
/// * `key` - Redis key for this synchronization barrier
/// * `swarm` - The libp2p swarm to drive
/// * `redis` - Redis client for inter-node coordination
/// * `target` - Number of nodes that must signal before proceeding
///
/// # Returns
/// All swarm events that occurred during the synchronization period
pub(crate) async fn barrier_and_drive_swarm(
    key: &str,
    swarm: &mut Swarm<Behaviour>,
    redis: &mut RedisClient,
    target: u64,
) -> Vec<SwarmEvent<Event>> {
    let swarm_events = swarm
        .take_until(redis.signal_and_wait(key, target).boxed_local())
        .collect::<Vec<_>>()
        .await;

    debug!("[Swarm events] {swarm_events:?}");
    swarm_events
}

/// Collects addresses from all test nodes for network setup.
///
/// Each node adds its address to a shared Redis list and waits until
/// all participating nodes have registered their addresses.
pub(crate) async fn collect_addr(
    redis: &mut RedisClient,
    local_addr: Multiaddr,
    target: usize,
) -> Vec<Multiaddr> {
    let key = "collect_addr";
    redis.push(key, local_addr.to_string()).await;

    let mut list = redis.list(key).await;
    loop {
        if list.len() >= target {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        list = redis.list(key).await;
    }

    list.iter()
        .map(|s| Multiaddr::try_from(s.as_str()).unwrap())
        .collect()
}

/// Ensures all connected peers have subscribed to the topic.
/// This function counts subscription events from previously received events
/// and waits for additional subscription events until the target number is reached.
///
/// Note: Currently only supports checking for a single topic.
pub(crate) async fn ensure_all_peers_subscribed(
    swarm: &mut Swarm<Behaviour>,
    received_events: &Vec<SwarmEvent<Event>>,
    target: usize,
) {
    let mut num_subscribed = 0;
    received_events.iter().for_each(|event| {
        if let SwarmEvent::Behaviour(Event::Subscribed { peer_id, topic }) = event {
            info!(
                "Peer {} successfully subscribed to topic '{}'",
                peer_id, topic
            );
            num_subscribed += 1;
        }
    });

    if num_subscribed == target {
        return;
    }

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(gossipsub_event) => match gossipsub_event {
                Event::Subscribed { peer_id, topic } => {
                    info!(
                        "Peer {} successfully subscribed to topic '{}'",
                        peer_id, topic
                    );
                    num_subscribed += 1;
                    if num_subscribed == target {
                        return;
                    }
                }
                _ => unreachable!(),
            },
            event => debug!("[Swarm event] {event:?}"),
        }
    }
}
