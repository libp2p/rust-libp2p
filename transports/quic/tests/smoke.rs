use anyhow::Result;
use async_trait::async_trait;
use futures::future::FutureExt;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use futures::stream::StreamExt;
use libp2p::core::upgrade;
use libp2p::request_response::{
    ProtocolName, ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::{Swarm, SwarmBuilder, SwarmEvent};
use libp2p_core::muxing::StreamMuxerBox;
use libp2p::{Multiaddr, Transport};
use libp2p_quic::{Config as QuicConfig, Endpoint as QuicEndpoint, QuicTransport};
use rand::RngCore;
use std::{io, iter};

use futures::task::Spawn;
use std::num::NonZeroU8;
use std::time::Duration;

fn generate_tls_keypair() -> libp2p::identity::Keypair {
    libp2p::identity::Keypair::generate_ed25519()
}

#[tracing::instrument]
async fn create_swarm(keylog: bool) -> Result<Swarm<RequestResponse<PingCodec>>> {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic".parse()?;
    let config = QuicConfig::new(&keypair, addr).unwrap();
    let endpoint = QuicEndpoint::new(config).unwrap();
    let transport = QuicTransport::new(endpoint);

    // TODO:
    // transport
    //     .transport
    //     .max_idle_timeout(Some(quinn_proto::VarInt::from_u32(1_000u32).into()));
    // if keylog {
    //     transport.enable_keylogger();
    // }

    let transport = 
        Transport::map(transport, |(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);
    tracing::info!(?peer_id);
    Ok(Swarm::new(transport, behaviour, peer_id))
}

fn setup_global_subscriber() -> impl Drop {
    use tracing_flame::FlameLayer;
    use tracing_subscriber::{prelude::*, fmt};

    let filter_layer = tracing_subscriber::EnvFilter::from_default_env();

    let fmt_format = tracing_subscriber::fmt::format()
        .pretty()
        .with_thread_ids(false)
        .without_time();
    let fmt_layer = fmt::Layer::default().event_format(fmt_format);

    let (flame_layer, _guard) = FlameLayer::with_file("./tracing.folded").unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(flame_layer)
        .try_init()
        .ok();
    _guard
}

#[async_std::test]
async fn smoke() -> Result<()> {
    let _guard = setup_global_subscriber();
    log_panics::init();
    let mut rng = rand::thread_rng();

    let mut a = create_swarm(true).await?;
    let mut b = create_swarm(false).await?;

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/udp/0/quic".parse()?)?;

    let addr = match a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };
    tracing::info!(?addr);

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

#[async_std::test]
async fn dial_failure() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();
    log_panics::init();

    let mut a = create_swarm(false).await?;
    let mut b = create_swarm(true).await?;

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/udp/0/quic".parse()?)?;

    let addr = match a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };
    let a_peer_id = &Swarm::local_peer_id(&a).clone();
    drop(a); // stop a swarm so b can never reach it

    b.behaviour_mut().add_address(a_peer_id, addr);
    b.behaviour_mut()
        .send_request(a_peer_id, Ping(b"hello world".to_vec()));

    match b.next().await {
        Some(SwarmEvent::Dialing(_)) => {}
        e => panic!("{:?}", e),
    }

    match b.next().await {
        Some(SwarmEvent::OutgoingConnectionError { .. }) => {}
        e => panic!("{:?}", e),
    };

    match b.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::OutboundFailure { .. })) => {}
        e => panic!("{:?}", e),
    };

    Ok(())
}

#[test]
fn concurrent_connections_and_streams() {
    use futures::executor::block_on;
    use quickcheck::*;

    tracing_subscriber::fmt()
        //.pretty()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();
    log_panics::init();

    #[tracing::instrument]
    fn prop(number_listeners: NonZeroU8, number_streams: NonZeroU8) -> TestResult {
        tracing::info!("entered");
        let (number_listeners, number_streams): (u8, u8) =
            (number_listeners.into(), number_streams.into());
        if number_listeners > 10 || number_streams > 10 {
            return TestResult::discard();
        }

        let mut pool = futures::executor::LocalPool::default();
        let mut data = vec![0; 4096 * 10];
        rand::thread_rng().fill_bytes(&mut data);
        let mut listeners = vec![];

        // Spawn the listener nodes.
        for _ in 0..number_listeners {
            let mut listener = block_on(create_swarm(true)).unwrap();
            Swarm::listen_on(&mut listener, "/ip4/127.0.0.1/udp/0/quic".parse().unwrap()).unwrap();

            // Wait to listen on address.
            let addr = match block_on(listener.next()) {
                Some(SwarmEvent::NewListenAddr { address, .. }) => address,
                e => panic!("{:?}", e),
            };

            listeners.push((*listener.local_peer_id(), addr));

            pool.spawner()
                .spawn_obj(
                    async move {
                        loop {
                            match listener.next().await {
                                Some(SwarmEvent::ConnectionEstablished { .. }) => {
                                    tracing::info!("listener ConnectionEstablished");
                                }
                                Some(SwarmEvent::IncomingConnection { .. }) => {
                                    tracing::info!("listener IncomingConnection");
                                }
                                Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
                                    message:
                                        RequestResponseMessage::Request {
                                            request: Ping(ping),
                                            channel,
                                            ..
                                        },
                                    ..
                                })) => {
                                    tracing::info!("listener got Message");
                                    listener
                                        .behaviour_mut()
                                        .send_response(channel, Pong(ping))
                                        .unwrap();
                                }
                                Some(SwarmEvent::Behaviour(
                                    RequestResponseEvent::ResponseSent { .. },
                                )) => {
                                    tracing::info!("listener ResponseSent");
                                }
                                Some(SwarmEvent::ConnectionClosed { .. }) => {}
                                Some(e) => {
                                    tracing::info!(?e, "listener");
                                } // panic!("{:?}", e),
                                None => {
                                    panic!("listener stopped");
                                }
                            }
                        }
                    }
                    .boxed()
                    .into(),
                )
                .unwrap();
        }

        let mut dialer = block_on(create_swarm(true)).unwrap();

        // For each listener node start `number_streams` requests.
        for (listener_peer_id, listener_addr) in &listeners {
            dialer
                .behaviour_mut()
                .add_address(&listener_peer_id, listener_addr.clone());

            dialer.dial(listener_peer_id.clone()).unwrap();
            //dialer
            //    .behaviour_mut()
            //    .send_request(&listener_peer_id, Ping(data.clone()));
        }

        // Wait for responses to each request.
        pool.run_until(async {
            let mut num_responses = 0;
            loop {
                match dialer.next().await {
                    Some(SwarmEvent::Dialing(_)) => {
                        tracing::info!("dialer Dialing");
                    }
                    Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                        tracing::info!("dialer Connection established");
                        for _ in 0..number_streams {
                            dialer
                                .behaviour_mut()
                                .send_request(&peer_id, Ping(data.clone()));
                        }
                    }
                    Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
                        message:
                            RequestResponseMessage::Response {
                                response: Pong(pong),
                                ..
                            },
                        ..
                    })) => {
                        tracing::info!("dialer got Message");
                        num_responses += 1;
                        assert_eq!(data, pong);
                        let should_be = number_listeners as usize * (number_streams) as usize;
                        tracing::info!(?num_responses, ?should_be);
                        if num_responses == should_be {
                            break;
                        }
                    }
                    Some(SwarmEvent::ConnectionClosed { .. }) => {
                        tracing::info!("dialer ConnectionClosed");
                    }
                    e => {
                        tracing::info!(?e, "dialer");
                    } // panic!("{:?}", e),
                }
            }
        });

        TestResult::passed()
    }

    QuickCheck::new().quickcheck(prop as fn(_, _) -> _);
}
