use std::{env, str::FromStr, time::Duration};

use anyhow::{Context, Result};
use env_logger::Env;
use log::info;
use redis::{AsyncCommands, Client as Rclient};
use strum::EnumString;

const REDIS_TIMEOUT: usize = 10;

/// Supported transports by rust-libp2p.
#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum Transport {
    Tcp,
    QuicV1,
    Webrtc,
    Ws,
}

/// Supported stream multiplexers by rust-libp2p.
#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum Muxer {
    Mplex,
    Yamux,
}

/// Supported security protocols by rust-libp2p.
#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum SecProtocol {
    Noise,
    Tls,
}

/// Helper function to get a ENV variable into an test parameter like `Transport`.
pub fn from_env<T>(env_var: &str) -> Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    env::var(env_var)
        .with_context(|| format!("{env_var} environment variable is not set"))?
        .parse()
        .map_err(Into::into)
}

/// PingSwarm allows us to abstract over libp2p versions for `run_ping`.
#[async_trait::async_trait]
pub trait PingSwarm: Sized + Send + 'static {
    async fn listen_on(&mut self, address: &str) -> Result<String>;

    fn dial(&mut self, address: &str) -> Result<()>;

    async fn await_connections(&mut self, number: usize);

    async fn await_pings(&mut self, number: usize) -> Vec<Duration>;

    async fn loop_on_next(&mut self);

    fn local_peer_id(&self) -> String;
}

/// Run a ping interop test. Based on `is_dialer`, either dial the address
/// retrieved via `listenAddr` key over the redis connection. Or wait to be pinged and have
/// `dialerDone` key ready on the redis connection.
pub async fn run_ping<S>(
    client: Rclient,
    mut swarm: S,
    local_addr: &str,
    local_peer_id: &str,
    is_dialer: bool,
) -> Result<()>
where
    S: PingSwarm,
{
    let mut conn = client.get_async_connection().await?;

    info!("Running ping test: {}", swarm.local_peer_id());
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!(
        "Test instance, listening for incoming connections on: {:?}.",
        local_addr
    );
    let local_addr = swarm.listen_on(local_addr).await?;

    if is_dialer {
        let result: Vec<String> = conn.blpop("listenerAddr", REDIS_TIMEOUT).await?;
        let other = result
            .get(1)
            .context("Failed to wait for listener to be ready")?;

        swarm.dial(other)?;
        info!("Test instance, dialing multiaddress on: {}.", other);

        swarm.await_connections(1).await;

        let results = swarm.await_pings(1).await;

        conn.rpush("dialerDone", "").await?;
        info!(
            "Ping successful: {:?}",
            results.first().expect("Should have a ping result")
        );
    } else {
        let ma = format!("{local_addr}/p2p/{local_peer_id}");
        conn.rpush("listenerAddr", ma).await?;

        // Drive Swarm in the background while we await for `dialerDone` to be ready.
        tokio::spawn(async move {
            swarm.loop_on_next().await;
        });

        let done: Vec<String> = conn.blpop("dialerDone", REDIS_TIMEOUT).await?;
        done.get(1)
            .context("Failed to wait for dialer conclusion")?;
        info!("Ping successful");
    }

    Ok(())
}
