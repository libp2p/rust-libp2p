use async_trait::async_trait;
use futures::future::FutureExt;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use futures::stream::StreamExt;
use futures::task::Spawn;

use libp2p::request_response::{
    ProtocolName, ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};

use libp2p::{
    core::{
        self,
        either::EitherOutput,
        upgrade::{self, InboundUpgradeExt, OptionalUpgrade, OutboundUpgradeExt, SelectUpgrade},
    },
    mplex, noise,
    plaintext::PlainText2Config,
    swarm::{Swarm, SwarmEvent},
    tcp, websocket, yamux, Transport,
};

use std::time::Duration;
use std::{io, iter};

fn generate_keypair() -> libp2p::identity::Keypair {
    libp2p::identity::Keypair::generate_ed25519()
}

#[tracing::instrument]
async fn create_swarm() -> Swarm<RequestResponse<PingCodec>> {
    let keypair = generate_keypair();
    let peer_id = keypair.public().to_peer_id();

    let transport = websocket::WsConfig::new(tcp::TcpTransport::new(tcp::GenTcpConfig::new()));

    let authentication_config = {
        let plaintext = PlainText2Config {
            local_public_key: keypair.public(),
        };

        SelectUpgrade::new(
            OptionalUpgrade::<noise::NoiseAuthenticated<noise::XX, noise::X25519Spec, ()>>::none(),
            OptionalUpgrade::some(plaintext),
        )
    };

    let multiplexing_config = {
        let mut mplex_config = mplex::MplexConfig::new();
        mplex_config.set_max_buffer_behaviour(mplex::MaxBufferBehaviour::Block);
        mplex_config.set_max_buffer_size(usize::MAX);

        let mut yamux_config = yamux::YamuxConfig::default();
        // Enable proper flow-control: window updates are only sent when
        // buffered data has been consumed.
        yamux_config.set_window_update_mode(yamux::WindowUpdateMode::on_read());

        core::upgrade::SelectUpgrade::new(yamux_config, mplex_config)
    };

    let transport = transport
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(
            authentication_config
                .map_inbound(move |result| match result {
                    EitherOutput::First((peer_id, o)) => (peer_id, EitherOutput::First(o)),
                    EitherOutput::Second((peer_id, o)) => (peer_id, EitherOutput::Second(o)),
                })
                .map_outbound(move |result| match result {
                    EitherOutput::First((peer_id, o)) => (peer_id, EitherOutput::First(o)),
                    EitherOutput::Second((peer_id, o)) => (peer_id, EitherOutput::Second(o)),
                }),
        )
        .multiplex(multiplexing_config)
        .timeout(Duration::from_secs(5))
        .boxed();

    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);
    tracing::info!(?peer_id);
    Swarm::new(transport, behaviour, peer_id)
}

fn setup_global_subscriber() {
    let filter_layer = tracing_subscriber::EnvFilter::from_default_env();
    tracing_subscriber::fmt()
        .with_env_filter(filter_layer)
        .try_init()
        .ok();
}

#[derive(Debug, Clone)]
struct PingProtocol();

#[derive(Clone)]
struct PingCodec();

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ping(Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pong(Vec<u8>);

impl ProtocolName for PingProtocol {
    fn protocol_name(&self) -> &[u8] {
        "/ping/1".as_bytes()
    }
}

#[async_trait]
impl RequestResponseCodec for PingCodec {
    type Protocol = PingProtocol;
    type Request = Ping;
    type Response = Pong;

    async fn read_request<T>(&mut self, _: &PingProtocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        upgrade::read_length_prefixed(io, 4096 * 10)
            .map(|res| match res {
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                Ok(vec) if vec.is_empty() => Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(vec) => Ok(Ping(vec)),
            })
            .await
    }

    async fn read_response<T>(&mut self, _: &PingProtocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        upgrade::read_length_prefixed(io, 4096 * 10)
            .map(|res| match res {
                Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                Ok(vec) if vec.is_empty() => Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(vec) => Ok(Pong(vec)),
            })
            .await
    }

    async fn write_request<T>(
        &mut self,
        _: &PingProtocol,
        io: &mut T,
        Ping(data): Ping,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        upgrade::write_length_prefixed(io, data).await?;
        io.close().await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &PingProtocol,
        io: &mut T,
        Pong(data): Pong,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        upgrade::write_length_prefixed(io, data).await?;
        io.close().await?;
        Ok(())
    }
}

#[test]
fn dial() {
    use futures::executor::block_on;

    setup_global_subscriber();

    let mut a = block_on(create_swarm());
    let mut b = block_on(create_swarm());

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/tcp/0/ws".parse().unwrap()).unwrap();

    let addr = match block_on(a.next()) {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };
    let a_peer_id = &Swarm::local_peer_id(&a).clone();

    b.behaviour_mut().add_address(a_peer_id, addr);
    b.behaviour_mut()
        .send_request(a_peer_id, Ping(b"hello world".to_vec()));

    let mut pool = futures::executor::LocalPool::default();

    pool.spawner()
        .spawn_obj(
            async move {
                while let Some(event) = a.next().await {
                    dbg!(&event);
                    match event {
                        SwarmEvent::ConnectionEstablished { .. } => {}
                        SwarmEvent::Behaviour(RequestResponseEvent::Message {
                            message:
                                RequestResponseMessage::Request {
                                    request: Ping(ping),
                                    channel,
                                    ..
                                },
                            ..
                        }) => {
                            a.behaviour_mut()
                                .send_response(channel, Pong(ping))
                                .unwrap();
                        }
                        _ => {}
                    }
                }
            }
            .boxed()
            .into(),
        )
        .unwrap();

    pool.run_until(async {
        while let Some(event) = b.next().await {
            dbg!(&event);
            match event {
                SwarmEvent::ConnectionEstablished { .. } => {}
                SwarmEvent::Behaviour(RequestResponseEvent::Message { .. }) => {
                    break;
                }
                _ => {}
            }
        }
    });
}
