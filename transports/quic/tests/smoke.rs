use anyhow::Result;
use async_trait::async_trait;
use futures::future::FutureExt;
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use futures::select;
use futures::stream::StreamExt;
use libp2p::core::multiaddr::Protocol;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::{upgrade, ConnectedPoint, Transport};
use libp2p::request_response::{
    ProtocolName, ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{DialError, Swarm, SwarmBuilder, SwarmEvent};
use libp2p_quic::{Config as QuicConfig, QuicTransport};
use rand::RngCore;
use std::num::NonZeroU8;
use std::{io, iter};

fn generate_tls_keypair() -> libp2p::identity::Keypair {
    libp2p::identity::Keypair::generate_ed25519()
}

#[tracing::instrument]
async fn create_swarm(keylog: bool) -> Result<Swarm<RequestResponse<PingCodec>>> {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let config = QuicConfig::new(&keypair).unwrap();
    let transport = QuicTransport::new(config);

    // TODO:
    // transport
    //     .transport
    //     .max_idle_timeout(Some(quinn_proto::VarInt::from_u32(1_000u32).into()));
    // if keylog {
    //     transport.enable_keylogger();
    // }

    let transport = Transport::map(transport, |(peer, muxer), _| {
        (peer, StreamMuxerBox::new(muxer))
    })
    .boxed();

    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);
    tracing::info!(?peer_id);
    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|f| {
            tokio::spawn(f);
        }))
        .build();
    Ok(swarm)
}

fn setup_global_subscriber() {
    let filter_layer = tracing_subscriber::EnvFilter::from_default_env();
    tracing_subscriber::fmt()
        .with_env_filter(filter_layer)
        .try_init()
        .ok();
}

#[tokio::test]
async fn smoke() -> Result<()> {
    setup_global_subscriber();
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

#[tokio::test]
async fn dial_failure() -> Result<()> {
    setup_global_subscriber();

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

#[tokio::test]
async fn concurrent_connections_and_streams() {
    use quickcheck::*;

    setup_global_subscriber();

    let number_listeners = 10;
    let number_streams = 10;

        let pool = tokio::runtime::Handle::current();
        let mut data = vec![0; 4096 * 10];
        rand::thread_rng().fill_bytes(&mut data);
        let mut listeners = vec![];

        // Spawn the listener nodes.
        for _ in 0..number_listeners {
            let mut listener = create_swarm(true).await.unwrap();
            Swarm::listen_on(&mut listener, "/ip4/127.0.0.1/udp/0/quic".parse().unwrap()).unwrap();

            // Wait to listen on address.
            let addr = match listener.next().await {
                Some(SwarmEvent::NewListenAddr { address, .. }) => address,
                e => panic!("{:?}", e),
            };

            listeners.push((*listener.local_peer_id(), addr));

            tokio::spawn(
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
                                panic!("unexpected event {:?}", e);
                            }
                            None => {
                                panic!("listener stopped");
                            }
                        }
                    }
                }
            );
        }

        let mut dialer = create_swarm(true).await.unwrap();

        // For each listener node start `number_streams` requests.
        for (listener_peer_id, listener_addr) in &listeners {
            dialer
                .behaviour_mut()
                .add_address(&listener_peer_id, listener_addr.clone());

            dialer.dial(listener_peer_id.clone()).unwrap();
        }

        // Wait for responses to each request.
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
                        panic!("unexpected event {:?}", e);
                    }
                }
            }
}

#[tokio::test]
async fn endpoint_reuse() -> Result<()> {
    setup_global_subscriber();

    let mut swarm_a = create_swarm(false).await?;
    let mut swarm_b = create_swarm(false).await?;
    let b_peer_id = *swarm_b.local_peer_id();

    swarm_a.listen_on("/ip4/127.0.0.1/udp/0/quic".parse()?)?;
    let a_addr = match swarm_a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    swarm_b.dial(a_addr.clone()).unwrap();
    let b_send_back_addr = loop {
        select! {
            ev = swarm_a.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished { endpoint, .. } => {
                    break endpoint.get_remote_address().clone()
                }
                SwarmEvent::IncomingConnection { local_addr, ..} => {
                    assert!(swarm_a.listeners().any(|a| a == &local_addr));
                }
                e => panic!("{:?}", e),
            },
            ev = swarm_b.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished { .. } => {},
                e => panic!("{:?}", e),
            }
        }
    };

    let dial_opts = DialOpts::peer_id(b_peer_id)
        .addresses(vec![b_send_back_addr.clone()])
        .extend_addresses_through_behaviour()
        .condition(PeerCondition::Always)
        .build();
    swarm_a.dial(dial_opts).unwrap();

    // Expect the dial to fail since b is not listening on an address.
    loop {
        select! {
            ev = swarm_a.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished { ..} => panic!("Unexpected dial success."),
                SwarmEvent::OutgoingConnectionError {error, .. } => {
                    assert!(matches!(error, DialError::Transport(_)));
                    break
                }
                _ => {}
            },
            _ = swarm_b.select_next_some() => {},
        }
    }
    swarm_b.listen_on("/ip4/127.0.0.1/udp/0/quic".parse()?)?;
    let b_addr = match swarm_b.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    let dial_opts = DialOpts::peer_id(b_peer_id)
        .addresses(vec![b_addr.clone(), b_send_back_addr])
        .condition(PeerCondition::Always)
        .build();
    swarm_a.dial(dial_opts).unwrap();
    let expected_b_addr = b_addr.with(Protocol::P2p(b_peer_id.into()));

    let mut a_reported = false;
    let mut b_reported = false;
    while !a_reported || !b_reported {
        select! {
            ev = swarm_a.select_next_some() => match ev{
                SwarmEvent::ConnectionEstablished { endpoint, ..} => {
                    assert!(endpoint.is_dialer());
                    assert_eq!(endpoint.get_remote_address(), &expected_b_addr);
                    a_reported = true;
                }
                SwarmEvent::OutgoingConnectionError {error,  .. } => {
                    panic!("Unexpected error {:}", error)
                }
                _ => {}
            },
            ev = swarm_b.select_next_some() => match ev{
                SwarmEvent::ConnectionEstablished { endpoint, ..} => {
                    match endpoint {
                        ConnectedPoint::Dialer{..} => panic!("Unexpected outbound connection"),
                        ConnectedPoint::Listener {send_back_addr, local_addr} => {
                            // Expect that the local listening endpoint was used for dialing.
                            assert!(swarm_b.listeners().any(|a| a == &local_addr));
                            assert_eq!(send_back_addr, a_addr);
                            b_reported = true;
                        }
                    }
                }
                _ => {}
            },
        }
    }

    Ok(())
}

#[tokio::test]
async fn ipv4_dial_ipv6() -> Result<()> {
    setup_global_subscriber();

    let mut swarm_a = create_swarm(false).await?;
    let mut swarm_b = create_swarm(false).await?;

    swarm_a.listen_on("/ip6/::1/udp/0/quic".parse()?)?;
    let a_addr = match swarm_a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    swarm_b.dial(a_addr.clone()).unwrap();

    loop {
        select! {
            ev = swarm_a.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished { .. } => {
                    return Ok(())
                }
                SwarmEvent::IncomingConnection { local_addr, ..} => {
                    assert!(swarm_a.listeners().any(|a| a == &local_addr));
                }
                e => panic!("{:?}", e),
            },
            ev = swarm_b.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished { .. } => {},
                e => panic!("{:?}", e),
            }
        }
    }
}
