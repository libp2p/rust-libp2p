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
    use either::Either;
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
    use tracing_subscriber::EnvFilter;

    use crate::{from_env, Muxer, SecProtocol, Transport};

    use super::BoxedTransport;

    pub(crate) type Instant = std::time::Instant;

    pub(crate) fn init_logger() {
        tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    }

    pub(crate) fn sleep(duration: Duration) -> BoxFuture<'static, ()> {
        tokio::time::sleep(duration).boxed()
    }

    fn muxer_protocol_from_env() -> Result<Either<yamux::Config, mplex::MplexConfig>> {
        Ok(match from_env("muxer")? {
            Muxer::Yamux => Either::Left(yamux::Config::default()),
            Muxer::Mplex => Either::Right(mplex::MplexConfig::new()),
        })
    }

    pub(crate) fn build_transport(
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
    use anyhow::{bail, Result};
    use futures::future::{BoxFuture, FutureExt};
    use libp2p::identity::Keypair;
    use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
    use libp2p::PeerId;
    use libp2p_webrtc_websys as webrtc;
    use std::time::Duration;

    use crate::{BlpopRequest, Transport};

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
    ) -> Result<(BoxedTransport, String)> {
        match transport {
            Transport::Webtransport => Ok((
                libp2p::webtransport_websys::Transport::new(
                    libp2p::webtransport_websys::Config::new(&local_key),
                )
                .boxed(),
                format!("/ip4/{ip}/udp/0/quic/webtransport"),
            )),
            Transport::WebRtcDirect => Ok((
                webrtc::Transport::new(webrtc::Config::new(&local_key)).boxed(),
                format!("/ip4/{ip}/udp/0/webrtc-direct"),
            )),
            _ => bail!("Only webtransport and webrtc-direct are supported with wasm"),
        }
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
