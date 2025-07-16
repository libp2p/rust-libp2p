use crate::redis::RedisClient;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::futures::{FutureExt, StreamExt};
use libp2p::gossipsub::{
    AllowAllSubscriptionFilter, Behaviour, Config, Event, IdentityTransform, MessageAuthenticity,
};
use libp2p::identity::Keypair;
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, Swarm, Transport};
use std::time::Duration;
use tracing::{debug, info};

pub(crate) fn gossipsub(keypair: Keypair) -> Result<Behaviour, &'static str> {
    Behaviour::new_with_subscription_filter_and_transform(
        MessageAuthenticity::Signed(keypair),
        Config::default(),
        AllowAllSubscriptionFilter {},
        IdentityTransform {},
    )
}

pub(crate) fn transport(
    keypair: &Keypair,
) -> libp2p::core::transport::Boxed<(libp2p::identity::PeerId, StreamMuxerBox)> {
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

pub(crate) async fn collect_addr(
    redis: &mut RedisClient,
    local_addr: Multiaddr,
    target: usize,
) -> Vec<Multiaddr> {
    const KEY: &str = "collect_addr";
    redis.push(KEY, local_addr.to_string()).await;

    let mut list = redis.list(KEY).await;
    loop {
        if list.len() >= target {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        list = redis.list(KEY).await;
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
