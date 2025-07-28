use crate::addr::get_listener_addr;
use crate::node::collect_addr;
use crate::redis::RedisClient;
use crate::subscribe::subscribe;
use libp2p::Multiaddr;
use tracing::info;

mod addr;
mod node;
mod redis;
mod subscribe;

#[tokio::main]
async fn main() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    let context = Context::new().await;
    info!("Context:");
    info!("  test_name: {}", context.test_name);
    info!("  node_count: {}", context.node_count);
    info!("  node_index: {}", context.node_index);
    info!("  local_addr: {}", context.local_addr);
    info!("  participants:");
    for (i, participant) in context.participants.iter().enumerate() {
        info!("    [{i}]: {participant}");
    }

    // Execute the specified test scenario
    match context.test_name.as_str() {
        "subscribe" => subscribe(context).await,
        test_name => unreachable!("Unknown test name: {test_name}"),
    };
}

/// Test execution context containing configuration and coordination resources.
///
/// Holds all necessary information for a test node including Redis connection
/// for inter-node coordination, network addresses, and test parameters.
struct Context {
    test_name: String,
    redis: RedisClient,
    local_addr: Multiaddr,
    participants: Vec<Multiaddr>,
    node_count: u64,
    node_index: u64,
}

impl Context {
    async fn new() -> Self {
        let redis_host = std::env::var("REDIS_HOST").expect("REDIS_HOST must be set");
        let redis_port = std::env::var("REDIS_PORT")
            .expect("REDIS_PORT must be set")
            .parse()
            .expect("REDIS_PORT must be a valid number");
        let mut redis = RedisClient::new(redis_host, redis_port)
            .await
            .expect("Connect to redis");

        let node_count: u64 = std::env::var("NODE_COUNT")
            .expect("NODE_COUNT must be set")
            .parse()
            .expect("NODE_COUNT must be a valid number");

        let local_addr = get_listener_addr(9000).unwrap();
        let participants = collect_addr(&mut redis, local_addr.clone(), node_count as usize).await;

        Context {
            test_name: std::env::var("TEST_NAME").expect("TEST_NAME must be set"),
            redis,
            local_addr,
            participants,
            node_count: std::env::var("NODE_COUNT")
                .expect("NODE_COUNT must be set")
                .parse()
                .expect("NODE_COUNT must be a valid number"),
            node_index: std::env::var("NODE_INDEX")
                .expect("NODE_INDEX must be set")
                .parse()
                .expect("NODE_INDEX must be a valid number"),
        }
    }
}
