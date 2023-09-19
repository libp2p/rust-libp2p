use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::PeerId;

// Native re-exports
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{build_transport, init_logger, sleep, swarm_builder, Instant, RedisClient};

// Wasm re-exports
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::{build_transport, init_logger, sleep, swarm_builder, Instant, RedisClient};

type BoxedTransport = Boxed<(PeerId, StreamMuxerBox)>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native {
    use std::time::Duration;

    use anyhow::{bail, Context, Result};
    use env_logger::{Env, Target};
    use futures::future::BoxFuture;
    use futures::FutureExt;
    use libp2p::core::muxing::StreamMuxerBox;
    use libp2p::core::upgrade::Version;
    use libp2p::identity::Keypair;
    use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
    use libp2p::websocket::WsConfig;
    use libp2p::{noise, quic, tcp, tls, yamux, PeerId, Transport as _};
    use libp2p_mplex as mplex;
    use libp2p_webrtc as webrtc;
    use redis::AsyncCommands;

    use crate::{Muxer, SecProtocol, Transport};

    use super::BoxedTransport;

    pub(crate) type Instant = std::time::Instant;

    pub(crate) fn init_logger() {
        env_logger::Builder::from_env(Env::default().default_filter_or("info"))
            .target(Target::Stdout)
            .init();
    }

    pub(crate) fn sleep(duration: Duration) -> BoxFuture<'static, ()> {
        tokio::time::sleep(duration).boxed()
    }

    pub(crate) fn build_transport(
        local_key: Keypair,
        ip: &str,
        transport: Transport,
        sec_protocol: Option<SecProtocol>,
        muxer: Option<Muxer>,
    ) -> Result<(BoxedTransport, String)> {
        let (transport, addr) = match (transport, sec_protocol, muxer) {
            (Transport::QuicV1, _, _) => (
                quic::tokio::Transport::new(quic::Config::new(&local_key))
                    .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
                    .boxed(),
                format!("/ip4/{ip}/udp/0/quic-v1"),
            ),
            (Transport::Tcp, Some(SecProtocol::Tls), Some(Muxer::Mplex)) => (
                tcp::tokio::Transport::new(tcp::Config::new())
                    .upgrade(Version::V1Lazy)
                    .authenticate(tls::Config::new(&local_key).context("failed to initialise tls")?)
                    .multiplex(mplex::MplexConfig::new())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0"),
            ),
            (Transport::Tcp, Some(SecProtocol::Tls), Some(Muxer::Yamux)) => (
                tcp::tokio::Transport::new(tcp::Config::new())
                    .upgrade(Version::V1Lazy)
                    .authenticate(tls::Config::new(&local_key).context("failed to initialise tls")?)
                    .multiplex(yamux::Config::default())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0"),
            ),
            (Transport::Tcp, Some(SecProtocol::Noise), Some(Muxer::Mplex)) => (
                tcp::tokio::Transport::new(tcp::Config::new())
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to intialise noise")?,
                    )
                    .multiplex(mplex::MplexConfig::new())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0"),
            ),
            (Transport::Tcp, Some(SecProtocol::Noise), Some(Muxer::Yamux)) => (
                tcp::tokio::Transport::new(tcp::Config::new())
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to intialise noise")?,
                    )
                    .multiplex(yamux::Config::default())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0"),
            ),
            (Transport::Ws, Some(SecProtocol::Tls), Some(Muxer::Mplex)) => (
                WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                    .upgrade(Version::V1Lazy)
                    .authenticate(tls::Config::new(&local_key).context("failed to initialise tls")?)
                    .multiplex(mplex::MplexConfig::new())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/ws"),
            ),
            (Transport::Ws, Some(SecProtocol::Tls), Some(Muxer::Yamux)) => (
                WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        tls::Config::new(&local_key).context("failed to intialise noise")?,
                    )
                    .multiplex(yamux::Config::default())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/ws"),
            ),
            (Transport::Ws, Some(SecProtocol::Noise), Some(Muxer::Mplex)) => (
                WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to initialise tls")?,
                    )
                    .multiplex(mplex::MplexConfig::new())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/ws"),
            ),
            (Transport::Ws, Some(SecProtocol::Noise), Some(Muxer::Yamux)) => (
                WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to intialise noise")?,
                    )
                    .multiplex(yamux::Config::default())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/ws"),
            ),
            (Transport::WebRtcDirect, _, _) => (
                webrtc::tokio::Transport::new(
                    local_key,
                    webrtc::tokio::Certificate::generate(&mut rand::thread_rng())?,
                )
                .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
                .boxed(),
                format!("/ip4/{ip}/udp/0/webrtc-direct"),
            ),
            (Transport::Webtransport, _, _) => bail!("Webtransport can only be used with wasm"),
            (Transport::Tcp | Transport::Ws, None, _) => {
                bail!("Missing security protocol for {transport:?}")
            }
            (Transport::Tcp | Transport::Ws, _, None) => {
                bail!("Missing muxer protocol for {transport:?}")
            }
        };
        Ok((transport, addr))
    }

    pub(crate) fn swarm_builder<TBehaviour: NetworkBehaviour>(
        transport: BoxedTransport,
        behaviour: TBehaviour,
        peer_id: PeerId,
    ) -> SwarmBuilder<TBehaviour> {
        SwarmBuilder::with_tokio_executor(transport, behaviour, peer_id)
    }

    pub(crate) struct RedisClient(redis::Client);

    impl RedisClient {
        pub(crate) fn new(redis_addr: &str) -> Result<Self> {
            Ok(Self(
                redis::Client::open(redis_addr).context("Could not connect to redis")?,
            ))
        }

        pub(crate) async fn blpop(&self, key: &str, timeout: u64) -> Result<Vec<String>> {
            let mut conn = self.0.get_async_connection().await?;
            Ok(conn.blpop(key, timeout as usize).await?)
        }

        pub(crate) async fn rpush(&self, key: &str, value: String) -> Result<()> {
            let mut conn = self.0.get_async_connection().await?;
            conn.rpush(key, value).await?;
            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm {
    use anyhow::{bail, Context, Result};
    use futures::future::{BoxFuture, FutureExt};
    use libp2p::core::upgrade::Version;
    use libp2p::identity::Keypair;
    use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
    use libp2p::{noise, yamux, PeerId, Transport as _};
    use libp2p_mplex as mplex;
    use libp2p_webrtc_websys as webrtc;
    use std::time::Duration;

    use crate::{BlpopRequest, Muxer, SecProtocol, Transport};

    use super::BoxedTransport;

    pub(crate) type Instant = instant::Instant;

    pub(crate) fn init_logger() {
        console_error_panic_hook::set_once();
        wasm_logger::init(wasm_logger::Config::default());
    }

    pub(crate) fn sleep(duration: Duration) -> BoxFuture<'static, ()> {
        futures_timer::Delay::new(duration).boxed()
    }

    pub(crate) fn build_transport(
        local_key: Keypair,
        ip: &str,
        transport: Transport,
        sec_protocol: Option<SecProtocol>,
        muxer: Option<Muxer>,
    ) -> Result<(BoxedTransport, String)> {
        Ok(match (transport, sec_protocol, muxer) {
            (Transport::Webtransport, _, _) => (
                libp2p::webtransport_websys::Transport::new(
                    libp2p::webtransport_websys::Config::new(&local_key),
                )
                .boxed(),
                format!("/ip4/{ip}/udp/0/quic/webtransport"),
            ),
            (Transport::Ws, Some(SecProtocol::Noise), Some(Muxer::Mplex)) => (
                libp2p::websocket_websys::Transport::default()
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to initialise noise")?,
                    )
                    .multiplex(mplex::MplexConfig::new())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/wss"),
            ),
            (Transport::Ws, Some(SecProtocol::Noise), Some(Muxer::Yamux)) => (
                libp2p::websocket_websys::Transport::default()
                    .upgrade(Version::V1Lazy)
                    .authenticate(
                        noise::Config::new(&local_key).context("failed to initialise noise")?,
                    )
                    .multiplex(yamux::Config::default())
                    .timeout(Duration::from_secs(5))
                    .boxed(),
                format!("/ip4/{ip}/tcp/0/wss"),
            ),
            (Transport::Ws, None, _) => {
                bail!("Missing security protocol for WS")
            }
            (Transport::Ws, Some(SecProtocol::Tls), _) => {
                bail!("TLS not supported in WASM")
            }
            (Transport::Ws, _, None) => {
                bail!("Missing muxer protocol for WS")
            }
            (Transport::WebRtcDirect, _, _) => (
                webrtc::Transport::new(webrtc::Config::new(&local_key)).boxed(),
                format!("/ip4/{ip}/udp/0/webrtc-direct"),
            ),
            (Transport::QuicV1 | Transport::Tcp, _, _) => {
                bail!("{transport:?} is not supported in WASM")
            }
        })
    }

    pub(crate) fn swarm_builder<TBehaviour: NetworkBehaviour>(
        transport: BoxedTransport,
        behaviour: TBehaviour,
        peer_id: PeerId,
    ) -> SwarmBuilder<TBehaviour> {
        SwarmBuilder::with_wasm_executor(transport, behaviour, peer_id)
    }

    pub(crate) struct RedisClient(String);

    impl RedisClient {
        pub(crate) fn new(base_url: &str) -> Result<Self> {
            Ok(Self(base_url.to_owned()))
        }

        pub(crate) async fn blpop(&self, key: &str, timeout: u64) -> Result<Vec<String>> {
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

        pub(crate) async fn rpush(&self, _: &str, _: String) -> Result<()> {
            bail!("unimplemented")
        }
    }
}
