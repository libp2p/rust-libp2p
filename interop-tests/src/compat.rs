use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::PeerId;

// Native re-exports
#[cfg(not(target_arch = "wasm32"))]
pub use native::build_transport;
#[cfg(not(target_arch = "wasm32"))]
pub use native::init_logger;
#[cfg(not(target_arch = "wasm32"))]
pub use native::sleep;
#[cfg(not(target_arch = "wasm32"))]
pub use native::swarm_builder;
#[cfg(not(target_arch = "wasm32"))]
pub use native::RedisClient;
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

// Wasm re-exports
#[cfg(target_arch = "wasm32")]
pub use wasm::build_transport;
#[cfg(target_arch = "wasm32")]
pub use wasm::init_logger;
#[cfg(target_arch = "wasm32")]
pub use wasm::sleep;
#[cfg(target_arch = "wasm32")]
pub use wasm::swarm_builder;
#[cfg(target_arch = "wasm32")]
pub use wasm::RedisClient;
#[cfg(target_arch = "wasm32")]
pub use wasm_timer::Instant;

/// Spawn an async task either with tokio or in wasm
#[macro_export]
macro_rules! spawn {
    ($fut:expr) => {{
        #[cfg(not(target_arch = "wasm32"))]
        let ret = tokio::spawn($fut);
        #[cfg(target_arch = "wasm32")]
        let ret = wasm_bindgen_futures::spawn_local($fut);
        ret
    }};
}

type BoxedTransport = Boxed<(PeerId, StreamMuxerBox)>;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::time::Duration;

    use anyhow::{bail, Context, Result};
    use either::Either;
    use env_logger::{Env, Target};
    use libp2p::core::muxing::StreamMuxerBox;
    use libp2p::core::upgrade::Version;
    use libp2p::identity::Keypair;
    use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
    use libp2p::websocket::WsConfig;
    use libp2p::{noise, tcp, tls, yamux, PeerId, Transport as _};
    use libp2p_mplex as mplex;
    use libp2p_quic as quic;
    use libp2p_webrtc as webrtc;
    use redis::AsyncCommands;

    use crate::{from_env, Muxer, SecProtocol, Transport};

    use super::BoxedTransport;

    pub async fn sleep(dur: Duration) {
        tokio::time::sleep(dur).await
    }

    pub fn init_logger() {
        env_logger::Builder::from_env(Env::default().default_filter_or("info"))
            .target(Target::Stdout)
            .init();
    }

    fn muxer_protocol_from_env() -> Result<Either<yamux::Config, mplex::MplexConfig>> {
        Ok(match from_env("muxer")? {
            Muxer::Yamux => Either::Left(yamux::Config::default()),
            Muxer::Mplex => Either::Right(mplex::MplexConfig::new()),
        })
    }

    pub fn build_transport(
        local_key: Keypair,
        ip: &str,
        transport: Transport,
    ) -> Result<(BoxedTransport, String)> {
        let (transport, addr) = match (transport, from_env::<SecProtocol>("security")) {
            (Transport::QuicV1, _) => (
                quic::tokio::Transport::new(quic::Config::new(&local_key))
                    .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
                    .boxed(),
                format!("/ip4/{ip}/udp/0/quic-v1"),
            ),
            (Transport::Tcp, Ok(SecProtocol::Tls)) => (
                tcp::tokio::Transport::new(tcp::Config::new())
                    .upgrade(Version::V1Lazy)
                    .authenticate(tls::Config::new(&local_key).context("failed to initialise tls")?)
                    .multiplex(muxer_protocol_from_env()?)
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0"),
            ),
            (Transport::Tcp, Ok(SecProtocol::Noise)) => (
                tcp::tokio::Transport::new(tcp::Config::new())
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to intialise noise")?,
                    )
                    .multiplex(muxer_protocol_from_env()?)
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0"),
            ),
            (Transport::Ws, Ok(SecProtocol::Tls)) => (
                WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                    .upgrade(Version::V1Lazy)
                    .authenticate(tls::Config::new(&local_key).context("failed to initialise tls")?)
                    .multiplex(muxer_protocol_from_env()?)
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/ws"),
            ),
            (Transport::Ws, Ok(SecProtocol::Noise)) => (
                WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to intialise noise")?,
                    )
                    .multiplex(muxer_protocol_from_env()?)
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/ws"),
            ),
            (Transport::WebRtcDirect, _) => (
                webrtc::tokio::Transport::new(
                    local_key,
                    webrtc::tokio::Certificate::generate(&mut rand::thread_rng())?,
                )
                .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
                .boxed(),
                format!("/ip4/{ip}/udp/0/webrtc-direct"),
            ),
            (Transport::Tcp, Err(_)) => bail!("Missing security protocol for TCP transport"),
            (Transport::Ws, Err(_)) => bail!("Missing security protocol for Websocket transport"),
            (Transport::Webtransport, _) => bail!("Webtransport can only be used with wasm"),
        };
        Ok((transport, addr))
    }

    pub fn swarm_builder<TBehaviour: NetworkBehaviour>(
        transport: BoxedTransport,
        behaviour: TBehaviour,
        peer_id: PeerId,
    ) -> SwarmBuilder<TBehaviour> {
        SwarmBuilder::with_tokio_executor(transport, behaviour, peer_id)
    }

    pub struct RedisClient(redis::Client);

    impl RedisClient {
        pub fn new(redis_addr: &str) -> Result<Self> {
            Ok(Self(
                redis::Client::open(redis_addr).context("Could not connect to redis")?,
            ))
        }

        pub async fn blpop(&self, key: &str, timeout: u64) -> Result<Vec<String>> {
            let mut conn = self.0.get_async_connection().await?;
            Ok(conn.blpop(key, timeout as usize).await?)
        }

        pub async fn rpush(&self, key: &str, value: String) -> Result<()> {
            let mut conn = self.0.get_async_connection().await?;
            conn.rpush(key, value).await?;
            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::time::Duration;

    use anyhow::{bail, Result};
    use libp2p::identity::Keypair;
    use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
    use libp2p::PeerId;

    use crate::{BlpopRequest, Transport};

    use super::BoxedTransport;

    pub async fn sleep(dur: Duration) {
        gloo_timers::future::TimeoutFuture::new(dur.as_millis() as u32).await
    }

    pub fn init_logger() {
        wasm_logger::init(wasm_logger::Config::default());
    }

    pub fn build_transport(
        local_key: Keypair,
        ip: &str,
        transport: Transport,
    ) -> Result<(BoxedTransport, String)> {
        if let Transport::Webtransport = transport {
            Ok((
                libp2p::webtransport_websys::Transport::new(
                    libp2p::webtransport_websys::Config::new(&local_key),
                )
                .boxed(),
                format!("/ip4/{ip}/udp/0/quic/webtransport"),
            ))
        } else {
            bail!("Only webtransport supported with wasm")
        }
    }

    pub fn swarm_builder<TBehaviour: NetworkBehaviour>(
        transport: BoxedTransport,
        behaviour: TBehaviour,
        peer_id: PeerId,
    ) -> SwarmBuilder<TBehaviour> {
        SwarmBuilder::with_wasm_executor(transport, behaviour, peer_id)
    }

    pub struct RedisClient(String);

    impl RedisClient {
        pub fn new(redis_proxy_addr: &str) -> Result<Self> {
            Ok(Self(redis_proxy_addr.to_owned()))
        }

        pub async fn blpop(&self, key: &str, timeout: u64) -> Result<Vec<String>> {
            let res = reqwest::Client::new()
                .post(&format!("http://{}/blpop", self.0))
                .json(&BlpopRequest {
                    key: key.to_owned(),
                    timeout,
                })
                .send()
                .await?
                .json()
                .await?;
            Ok(res)
        }

        pub async fn rpush(&self, _key: &str, _value: String) -> Result<()> {
            unimplemented!("Wasm implementation can only be ran as a dialer")
        }
    }
}
