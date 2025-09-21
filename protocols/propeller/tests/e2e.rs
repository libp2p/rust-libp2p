//! End-to-end tests with large networks and leader rotation.

use std::{collections::HashMap, time::Duration};

use futures::{stream::SelectAll, StreamExt};
use libp2p_identity::PeerId;
use libp2p_propeller::{Behaviour, Config, Event, MessageAuthenticity, MessageId};
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use rand::{Rng, SeedableRng};
use rstest::rstest;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// ****************************************************************************

/// Transport type for testing
#[derive(Debug, Clone, Copy)]
enum TransportType {
    Memory,
    Quic,
}

/// A custom tracing layer that panics when an ERROR level event is logged
struct FailOnErrorLayer;

impl<S> tracing_subscriber::Layer<S> for FailOnErrorLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if *event.metadata().level() == tracing::Level::ERROR {
            // Format the error message
            struct ErrorMessageVisitor {
                message: String,
            }

            impl tracing::field::Visit for ErrorMessageVisitor {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    use std::fmt::Write;
                    if !self.message.is_empty() {
                        let _ = write!(&mut self.message, ", ");
                    }
                    let _ = write!(&mut self.message, "{}={:?}", field.name(), value);
                }
            }

            let mut visitor = ErrorMessageVisitor {
                message: String::new(),
            };
            event.record(&mut visitor);

            panic!(
                "ERROR level log detected at {}:{} - {}",
                event.metadata().file().unwrap_or("unknown"),
                event.metadata().line().unwrap_or(0),
                visitor.message
            );
        }
    }
}

async fn create_swarm(
    fanout: usize,
    fec_data_shreds: usize,
    fec_coding_shreds: usize,
    transport_type: TransportType,
) -> Swarm<Behaviour> {
    use libp2p_core::{upgrade::Version, Transport as _};
    use libp2p_identity::Keypair;

    let config = Config::builder()
        .fanout(fanout)
        .emit_shred_received_events(true)
        .fec_coding_shreds(fec_coding_shreds)
        .fec_data_shreds(fec_data_shreds)
        .validation_mode(libp2p_propeller::ValidationMode::None)
        .max_shred_size(1 << 24) // 16MB
        .reconstructed_messages_ttl(Duration::from_secs(3600)) // 1 hour
        .substream_timeout(Duration::from_secs(300)) // Increased timeout for large message tests
        .build();

    let identity = Keypair::generate_ed25519();
    let peer_id = PeerId::from(identity.public());

    let transport = match transport_type {
        TransportType::Memory => libp2p_core::transport::MemoryTransport::default()
            .or_transport(libp2p_tcp::tokio::Transport::default())
            .upgrade(Version::V1)
            .authenticate(libp2p_plaintext::Config::new(&identity))
            .multiplex(libp2p_yamux::Config::default())
            .timeout(Duration::from_secs(300))
            .boxed(),
        TransportType::Quic => {
            let mut quic_config = libp2p_quic::Config::new(&identity);
            quic_config.max_connection_data = u32::MAX;
            quic_config.max_stream_data = u32::MAX;
            quic_config.keep_alive_interval = Duration::from_secs(300);
            quic_config.max_idle_timeout = 300 * 1000;
            libp2p_quic::tokio::Transport::new(quic_config)
                .map(|(peer_id, muxer), _| {
                    (peer_id, libp2p_core::muxing::StreamMuxerBox::new(muxer))
                })
                .boxed()
        }
    };

    // Use a much longer idle connection timeout to prevent disconnections during long tests
    let swarm_config = libp2p_swarm::Config::with_tokio_executor()
        .with_idle_connection_timeout(Duration::from_secs(3600)); // 1 hour

    Swarm::new(
        transport,
        Behaviour::new(MessageAuthenticity::Signed(identity), config),
        peer_id,
        swarm_config,
    )
}

async fn setup_network(
    num_nodes: usize,
    fanout: usize,
    fec_data_shreds: usize,
    fec_coding_shreds: usize,
    transport_type: TransportType,
) -> (Vec<Swarm<Behaviour>>, Vec<PeerId>) {
    let mut swarms = Vec::with_capacity(num_nodes);
    let mut peer_ids = Vec::with_capacity(num_nodes);

    for _ in 0..num_nodes {
        let mut swarm =
            create_swarm(fanout, fec_data_shreds, fec_coding_shreds, transport_type).await;
        let peer_id = *swarm.local_peer_id();

        match transport_type {
            TransportType::Memory => {
                swarm.listen().with_memory_addr_external().await;
            }
            TransportType::Quic => {
                swarm
                    .listen_on("/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap())
                    .unwrap();
                // Wait for the listening event and add as external address
                loop {
                    if let SwarmEvent::NewListenAddr { address, .. } =
                        swarm.select_next_some().await
                    {
                        swarm.add_external_address(address);
                        break;
                    }
                }
            }
        }

        peer_ids.push(peer_id);
        swarms.push(swarm);
    }

    tracing::info!("🔗 Connecting all peers");
    connect_all_peers(&mut swarms).await;
    tracing::info!("🔗 Adding all peers");
    add_all_peers(&mut swarms, &peer_ids);

    (swarms, peer_ids)
}

async fn connect_all_peers(swarms: &mut [Swarm<Behaviour>]) {
    let num_nodes = swarms.len();

    for i in 0..num_nodes {
        for j in (i + 1)..num_nodes {
            let (left, right) = swarms.split_at_mut(j);
            let swarm_i = &mut left[i];
            let swarm_j = &mut right[0];

            tracing::info!("connecting swarm {i} to {j}");
            swarm_j.connect(swarm_i).await;
        }
    }
}

fn add_all_peers(swarms: &mut [Swarm<Behaviour>], peer_ids: &[PeerId]) {
    let peer_weights: Vec<(PeerId, u64)> =
        peer_ids.iter().map(|&peer_id| (peer_id, 1000)).collect();

    for swarm in swarms.iter_mut() {
        let _ = swarm.behaviour_mut().set_peers(peer_weights.clone());
    }
}

async fn collect_message_events(
    swarms: &mut [Swarm<Behaviour>],
    expected_message_ids: Vec<MessageId>,
    number_of_messages: usize,
    number_of_shreds: usize,
    leader_idx: usize,
    early_stop: bool,
) -> (
    HashMap<(usize, MessageId), Vec<u8>>,
    HashMap<(usize, MessageId, u32), Vec<u8>>,
) {
    let mut received_messages: HashMap<(usize, MessageId), Vec<u8>> = HashMap::new();
    let mut received_shreds: HashMap<(usize, MessageId, u32), Vec<u8>> = HashMap::new();
    tracing::info!("🔍 Collecting events, need {} messages", number_of_messages);

    // Create a SelectAll to efficiently poll all swarm streams
    let mut select_all = SelectAll::new();

    // Add each swarm's stream with its index
    for (node_idx, swarm) in swarms.iter_mut().enumerate() {
        let stream = swarm.map(move |event| (node_idx, event));
        select_all.push(stream);
    }

    while let Some((node_idx, swarm_event)) = select_all.next().await {
        if let Ok(event) = swarm_event.try_into_behaviour_event() {
            match event {
                Event::ShredReceived { sender: _, shred } => {
                    if !expected_message_ids.contains(&shred.id.message_id) {
                        continue;
                    }
                    if received_shreds.contains_key(&(
                        node_idx,
                        shred.id.message_id,
                        shred.id.index,
                    )) {
                        panic!(
                            "🚨 DUPLICATE SHRED: Node {} received a duplicate shred! This should not happen. message_id={}, index={}",
                            node_idx, shred.id.message_id, shred.id.index
                        );
                    }
                    received_shreds
                        .insert((node_idx, shred.id.message_id, shred.id.index), shred.shard);
                    tracing::info!(
                        "📨 Node {} received shred for message_id={} index={} ({}/{})",
                        node_idx,
                        shred.id.message_id,
                        shred.id.index,
                        received_shreds.len(),
                        number_of_shreds,
                    );
                    if number_of_shreds == received_shreds.len() {
                        break;
                    }
                }
                Event::MessageReceived {
                    publisher: _,
                    data,
                    message_id,
                } => {
                    if !expected_message_ids.contains(&message_id) {
                        continue;
                    }
                    if received_messages.contains_key(&(node_idx, message_id)) {
                        panic!(
                                "🚨 DUPLICATE MESSAGE: Node {} received a duplicate message! This should not happen. message_id: {}",
                                node_idx, message_id
                            );
                    }
                    assert!(received_messages.len() < number_of_messages);
                    assert_ne!(node_idx, leader_idx);
                    received_messages.insert((node_idx, message_id), data);
                    tracing::info!(
                        "📨 Node {} received message {} ({}/{})",
                        node_idx,
                        message_id,
                        received_messages.len(),
                        number_of_messages
                    );
                    if received_messages.len() == number_of_messages && early_stop {
                        break;
                    }
                }
                Event::ShredSendFailed {
                    sent_from: _,
                    sent_to,
                    error,
                } => {
                    panic!(
                        "Node {} failed to send shred to peer {:?}: {}",
                        node_idx, sent_to, error
                    );
                }
                Event::ShredValidationFailed {
                    sender,
                    shred_id: _,
                    error,
                } => {
                    panic!(
                        "Node {} failed to verify shred from peer {}: {}",
                        node_idx, sender, error
                    );
                }
                Event::MessageReconstructionFailed {
                    message_id,
                    publisher,
                    error,
                } => {
                    panic!(
                            "Node {} failed to reconstruct message from shreds: publisher={}, message_id={}, error={}",
                            node_idx, publisher, message_id, error
                        );
                }
            }
        }
    }

    (received_messages, received_shreds)
}

fn broadcast_message(
    swarms: &mut [Swarm<Behaviour>],
    leader_idx: usize,
    message_id: MessageId,
    test_message: &[u8],
) -> HashMap<(usize, MessageId, u32), Vec<u8>> {
    tracing::info!(
        "📡 Leader {} broadcasting message {} of {} bytes",
        leader_idx,
        message_id,
        test_message.len()
    );

    let mut test_shreds = HashMap::new();
    let shreds = swarms[leader_idx]
        .behaviour_mut()
        .broadcast(test_message.to_vec(), message_id)
        .unwrap();

    for shred in shreds {
        test_shreds.insert((leader_idx, message_id, shred.id.index), shred.shard);
    }

    test_shreds
}

fn verify_received_data(
    received_messages: HashMap<(usize, MessageId), Vec<u8>>,
    received_shreds: HashMap<(usize, MessageId, u32), Vec<u8>>,
    test_messages: &HashMap<MessageId, Vec<u8>>,
    test_shreds: &HashMap<(usize, MessageId, u32), Vec<u8>>,
    leader_idx: usize,
) {
    for ((node_idx, message_id), message) in received_messages {
        let test_message = test_messages.get(&message_id).unwrap();
        assert_eq!(
            &message, test_message,
            "Node {} received incorrect reconstructed message from leader {}: message_id={}",
            node_idx, leader_idx, message_id
        );
    }

    for ((node_idx, message_id, index), shred) in received_shreds {
        let test_shred = test_shreds.get(&(leader_idx, message_id, index)).unwrap();
        assert_eq!(
            &shred, test_shred,
            "Node {} received incorrect shred from leader {}: message_id={}, index={}",
            node_idx, leader_idx, message_id, index
        );
    }
}

fn assert_collection_counts(
    received_messages: &HashMap<(usize, MessageId), Vec<u8>>,
    received_shreds: &HashMap<(usize, MessageId, u32), Vec<u8>>,
    expected_messages: usize,
    expected_shreds: usize,
    early_stop: bool,
) {
    assert_eq!(received_messages.len(), expected_messages);
    if !early_stop {
        assert_eq!(received_shreds.len(), expected_shreds);
    }
}

#[allow(clippy::too_many_arguments)]
async fn broadcast_and_verify_burst(
    swarms: &mut [Swarm<Behaviour>],
    test_messages: &HashMap<MessageId, Vec<u8>>,
    leader_idx: usize,
    num_nodes: usize,
    number_of_messages: usize,
    fec_data_shreds: usize,
    fec_coding_shreds: usize,
    early_stop: bool,
) {
    tracing::info!("📤 Broadcasting all messages in burst mode");
    let mut test_shreds = HashMap::new();

    for (message_id, test_message) in test_messages.iter() {
        let shreds = broadcast_message(swarms, leader_idx, *message_id, test_message);
        test_shreds.extend(shreds);
    }

    tracing::info!("⏳ Collecting message events for leader {}...", leader_idx);
    let (received_messages, received_shreds) = collect_message_events(
        swarms,
        test_messages.keys().cloned().collect(),
        number_of_messages * (num_nodes - 1),
        number_of_messages * (num_nodes - 1) * (fec_data_shreds + fec_coding_shreds),
        leader_idx,
        early_stop,
    )
    .await;

    assert_collection_counts(
        &received_messages,
        &received_shreds,
        number_of_messages * (num_nodes - 1),
        number_of_messages * (num_nodes - 1) * (fec_data_shreds + fec_coding_shreds),
        early_stop,
    );

    verify_received_data(
        received_messages,
        received_shreds,
        test_messages,
        &test_shreds,
        leader_idx,
    );
}

async fn broadcast_and_verify_sequential(
    swarms: &mut [Swarm<Behaviour>],
    test_messages: &HashMap<MessageId, Vec<u8>>,
    leader_idx: usize,
    num_nodes: usize,
    fec_data_shreds: usize,
    fec_coding_shreds: usize,
    early_stop: bool,
) {
    tracing::info!("📤 Broadcasting messages sequentially with verification");

    for (message_id, test_message) in test_messages.iter() {
        tracing::info!(
            "📡 Leader {} broadcasting message {} of {} bytes (sequential mode)",
            leader_idx,
            message_id,
            test_message.len()
        );

        let test_shreds = broadcast_message(swarms, leader_idx, *message_id, test_message);

        tracing::info!("⏳ Collecting events for message {}...", message_id);
        let (received_messages, received_shreds) = collect_message_events(
            swarms,
            vec![*message_id],
            num_nodes - 1,
            (num_nodes - 1) * (fec_data_shreds + fec_coding_shreds),
            leader_idx,
            early_stop,
        )
        .await;

        assert_collection_counts(
            &received_messages,
            &received_shreds,
            num_nodes - 1,
            (num_nodes - 1) * (fec_data_shreds + fec_coding_shreds),
            early_stop,
        );

        let mut single_message = HashMap::new();
        single_message.insert(*message_id, test_message.clone());

        verify_received_data(
            received_messages,
            received_shreds,
            &single_message,
            &test_shreds,
            leader_idx,
        );

        tracing::info!("✅ Message {} verified successfully", message_id);
    }
}

/// Initialize the tracing subscriber with error detection
fn init_tracing(env_filter: EnvFilter) {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .with(FailOnErrorLayer)
            .init();
    });
}

#[allow(clippy::too_many_arguments)]
async fn e2e(
    num_nodes: usize,
    fanout: usize,
    fec_data_shreds: usize,
    fec_coding_shreds: usize,
    number_of_messages: usize,
    number_of_leaders: usize,
    message_size: usize,
    early_stop: bool,
    send_in_burst: bool,
    transport_type: TransportType,
) {
    let (mut swarms, peer_ids) = setup_network(
        num_nodes,
        fanout,
        fec_data_shreds,
        fec_coding_shreds,
        transport_type,
    )
    .await;
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    for leader_idx in (0..num_nodes).step_by(num_nodes / number_of_leaders) {
        tracing::info!("🔄 Starting rotation to leader {}", leader_idx);

        let leader_peer_id = peer_ids[leader_idx];
        tracing::info!("🎯 Setting leader to peer_id: {}", leader_peer_id);
        tracing::info!("✅ Leader {} confirmed", leader_idx);

        tracing::info!("🔄 Creating test messages");
        let mut test_messages = HashMap::new();
        for _ in 0..number_of_messages {
            let message: Vec<_> = (0..message_size).map(|_| rng.random::<u8>()).collect();
            let message_id = rng.random::<u64>();
            test_messages.insert(message_id, message);
        }

        // Check connection status before broadcasting
        let connected_count = swarms[leader_idx].connected_peers().count();
        tracing::info!(
            "🔗 Leader {} has {} connected peers",
            leader_idx,
            connected_count
        );

        if send_in_burst {
            broadcast_and_verify_burst(
                &mut swarms,
                &test_messages,
                leader_idx,
                num_nodes,
                number_of_messages,
                fec_data_shreds,
                fec_coding_shreds,
                early_stop,
            )
            .await;
        } else {
            broadcast_and_verify_sequential(
                &mut swarms,
                &test_messages,
                leader_idx,
                num_nodes,
                fec_data_shreds,
                fec_coding_shreds,
                early_stop,
            )
            .await;
        }

        tracing::info!("✅ ✅ ✅ Leader {} broadcast successful", leader_idx);
    }
}

#[tokio::test]
async fn random_e2e_test_memory() {
    init_tracing(
        EnvFilter::builder()
            // .with_default_directive(tracing::level_filters::LevelFilter::INFO.into())
            .from_env_lossy(),
    );

    const NUM_TESTS: u64 = 10;
    for i in 0..NUM_TESTS {
        let seed = rand::random();
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let num_nodes = rng.random_range(2..=100);
        let fanout = rng.random_range(1..=10);
        let fec_data_shreds = rng.random_range(1..=10);
        let fec_coding_shreds = rng.random_range(1..=10);
        let number_of_messages = rng.random_range(1..=2);
        let number_of_leaders = rng.random_range(1..=2);
        let message_size = fec_data_shreds * 2 * rng.random_range(1..=1024);
        let early_stop = rng.random_bool(0.5);
        let send_in_burst = rng.random_bool(0.5);
        println!("Memory {}: Running test with seed {}: num_nodes={}, fanout={}, fec_data_shreds={}, fec_coding_shreds={}, number_of_messages={}, number_of_leaders={}, message_size={}, early_stop={}, send_in_burst={}",
         i, seed, num_nodes, fanout, fec_data_shreds, fec_coding_shreds, number_of_messages, number_of_leaders, message_size, early_stop, send_in_burst,
        );
        e2e(
            num_nodes,
            fanout,
            fec_data_shreds,
            fec_coding_shreds,
            number_of_messages,
            number_of_leaders,
            message_size,
            early_stop,
            send_in_burst,
            TransportType::Memory,
        )
        .await;
    }
}

#[tokio::test]
async fn random_e2e_test_quic() {
    init_tracing(
        EnvFilter::builder()
            // .with_default_directive(tracing::level_filters::LevelFilter::INFO.into())
            .from_env_lossy(),
    );

    const NUM_TESTS: u64 = 10;
    for i in 0..NUM_TESTS {
        let seed = rand::random();
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let num_nodes = rng.random_range(2..=10); // quic is slower
        let fanout = rng.random_range(1..=10);
        let fec_data_shreds = rng.random_range(1..=10);
        let fec_coding_shreds = rng.random_range(1..=10);
        let number_of_messages = rng.random_range(1..=2);
        let number_of_leaders = rng.random_range(1..=2);
        let message_size = fec_data_shreds * 2 * rng.random_range(1..=3);
        let early_stop = rng.random_bool(0.5);
        let send_in_burst = rng.random_bool(0.5);
        println!("QUIC {}: Running test with seed {}: num_nodes={}, fanout={}, fec_data_shreds={}, fec_coding_shreds={}, number_of_messages={}, number_of_leaders={}, message_size={}, early_stop={}, send_in_burst={}",
         i, seed, num_nodes, fanout, fec_data_shreds, fec_coding_shreds, number_of_messages, number_of_leaders, message_size, early_stop, send_in_burst,
        );
        e2e(
            num_nodes,
            fanout,
            fec_data_shreds,
            fec_coding_shreds,
            number_of_messages,
            number_of_leaders,
            message_size,
            early_stop,
            send_in_burst,
            TransportType::Quic,
        )
        .await;
    }
}

#[tokio::test]
#[rstest]
#[case(1<<10, 100)]
#[case(1<<11, 100)]
#[case(1<<12, 100)]
#[case(1<<13, 100)]
#[case(1<<14, 100)]
#[case(1<<15, 100)]
#[case(1<<16, 100)]
#[case(1<<17, 100)]
#[case(1<<18, 100)]
#[case(1<<19, 100)]
#[case(1<<20, 100)]
#[case(1<<21, 100)]
#[case(1<<22, 50)] // runs too long in non-release mode for 100 nodes
#[case(1<<23, 25)]
async fn specific_e2e_message_sizes(#[case] message_size: usize, #[case] num_nodes: usize) {
    init_tracing(
        EnvFilter::builder()
            // .with_default_directive(LevelFilter::INFO.into())
            .from_env_lossy(),
    );

    let default_config = Config::builder().build();

    e2e(
        num_nodes,
        default_config.fanout(),
        default_config.fec_data_shreds(),
        default_config.fec_coding_shreds(),
        1,
        1,
        message_size,
        true,
        true, // send_in_burst
        TransportType::Memory,
    )
    .await;
}
