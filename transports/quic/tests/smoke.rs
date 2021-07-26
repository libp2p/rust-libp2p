use anyhow::Result;
use async_trait::async_trait;
use futures::future::FutureExt;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use futures::stream::StreamExt;
use libp2p::core::upgrade;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{
    ProtocolName, ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p_quic::{Crypto, Keypair, QuicConfig, ToLibp2p};
use quinn_proto::crypto::Session;
use rand::RngCore;
use std::{io, iter};

#[cfg(feature = "noise")]
#[async_std::test]
async fn smoke_noise() -> Result<()> {
    smoke::<libp2p_quic::NoiseCrypto>().await
}

#[cfg(feature = "tls")]
#[async_std::test]
async fn smoke_tls() -> Result<()> {
    smoke::<libp2p_quic::TlsCrypto>().await
}

async fn create_swarm<C: Crypto>(keylog: bool) -> Result<Swarm<RequestResponse<PingCodec>>>
where
    <C::Session as Session>::ClientConfig: Send + Unpin,
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    let keypair = Keypair::generate(&mut rand_core::OsRng {});
    let peer_id = keypair.to_peer_id();
    let mut transport = QuicConfig::<C>::new(keypair);
    if keylog {
        transport.enable_keylogger();
    }
    let transport = transport
        .listen_on("/ip4/127.0.0.1/udp/0/quic".parse()?)
        .await?
        .boxed();

    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);
    tracing::info!("{}", peer_id);
    Ok(Swarm::new(transport, behaviour, peer_id))
}

async fn smoke<C: Crypto>() -> Result<()>
where
    <C::Session as Session>::ClientConfig: Send + Unpin,
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();
    log_panics::init();
    let mut rng = rand::thread_rng();

    let mut a = create_swarm::<C>(true).await?;
    let mut b = create_swarm::<C>(false).await?;

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/udp/0/quic".parse()?)?;

    let mut addr = match a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };
    addr.push(Protocol::P2p((*a.local_peer_id()).into()));

    let mut data = vec![0; 4096 * 10];
    rng.fill_bytes(&mut data);

    b.behaviour_mut()
        .add_address(&Swarm::local_peer_id(&a), addr);
    b.behaviour_mut()
        .send_request(&Swarm::local_peer_id(&a), Ping(data.clone()));

    match b.next().await {
        Some(SwarmEvent::Dialing(_)) => {}
        e => panic!("{:?}", e),
    }

    match a.next().await {
        Some(SwarmEvent::IncomingConnection { .. }) => {}
        e => panic!("{:?}", e),
    };

    match b.next().await {
        Some(SwarmEvent::ConnectionEstablished { .. }) => {}
        e => panic!("{:?}", e),
    };

    match a.next().await {
        Some(SwarmEvent::ConnectionEstablished { .. }) => {}
        e => panic!("{:?}", e),
    };

    assert!(b.next().now_or_never().is_none());

    match a.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
            message:
                RequestResponseMessage::Request {
                    request: Ping(ping),
                    channel,
                    ..
                },
            ..
        })) => {
            a.behaviour_mut()
                .send_response(channel, Pong(ping))
                .unwrap();
        }
        e => panic!("{:?}", e),
    }

    match a.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::ResponseSent { .. })) => {}
        e => panic!("{:?}", e),
    }

    match b.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
            message:
                RequestResponseMessage::Response {
                    response: Pong(pong),
                    ..
                },
            ..
        })) => assert_eq!(data, pong),
        e => panic!("{:?}", e),
    }

    a.behaviour_mut().send_request(
        &Swarm::local_peer_id(&b),
        Ping(b"another substream".to_vec()),
    );

    assert!(a.next().now_or_never().is_none());

    match b.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
            message:
                RequestResponseMessage::Request {
                    request: Ping(data),
                    channel,
                    ..
                },
            ..
        })) => {
            b.behaviour_mut()
                .send_response(channel, Pong(data))
                .unwrap();
        }
        e => panic!("{:?}", e),
    }

    match b.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::ResponseSent { .. })) => {}
        e => panic!("{:?}", e),
    }

    match a.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
            message:
                RequestResponseMessage::Response {
                    response: Pong(data),
                    ..
                },
            ..
        })) => assert_eq!(data, b"another substream".to_vec()),
        e => panic!("{:?}", e),
    }

    Ok(())
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
