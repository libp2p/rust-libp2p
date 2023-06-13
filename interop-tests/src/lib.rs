use std::str::FromStr;

use anyhow::{bail, Context, Result};
use libp2p::ping;
use libp2p::swarm::{keep_alive, NetworkBehaviour};

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use native::run_test;
#[cfg(target_arch = "wasm32")]
pub use wasm::run_test;

/// A request to redis proxy that will pop the value from the list
/// and will wait for it being inserted until a timeout is reached.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct BlpopRequest {
    pub key: String,
    pub timeout: u64,
}

/// Supported transports by rust-libp2p.
#[derive(Clone, Debug)]
pub enum Transport {
    Tcp,
    QuicV1,
    WebRtcDirect,
    Ws,
    Webtransport,
}

impl FromStr for Transport {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "tcp" => Self::Tcp,
            "quic-v1" => Self::QuicV1,
            "webrtc-direct" => Self::WebRtcDirect,
            "ws" => Self::Ws,
            "webtransport" => Self::Webtransport,
            other => bail!("unknown transport {other}"),
        })
    }
}

/// Supported stream multiplexers by rust-libp2p.
#[derive(Clone, Debug)]
pub enum Muxer {
    Mplex,
    Yamux,
}

impl FromStr for Muxer {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "mplex" => Self::Mplex,
            "yamux" => Self::Yamux,
            other => bail!("unknown muxer {other}"),
        })
    }
}

/// Supported security protocols by rust-libp2p.
#[derive(Clone, Debug)]
pub enum SecProtocol {
    Noise,
    Tls,
}

impl FromStr for SecProtocol {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "noise" => Self::Noise,
            "tls" => Self::Tls,
            other => bail!("unknown security protocol {other}"),
        })
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

/// Helper function to get a ENV variable into an test parameter like `Transport`.
pub fn from_env<T>(env_var: &str) -> Result<T>
where
    T: FromStr<Err = anyhow::Error>,
{
    std::env::var(env_var)
        .with_context(|| format!("{env_var} environment variable is not set"))?
        .parse()
        .map_err(Into::into)
}
