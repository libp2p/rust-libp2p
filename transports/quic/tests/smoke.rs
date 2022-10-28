#![cfg(any(feature = "async-std", feature = "tokio"))]

use async_trait::async_trait;
use futures::channel::oneshot;
use futures::future::{join, FutureExt};
use futures::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use futures::select;
use futures::stream::StreamExt;
use futures::task::Spawn;
use libp2p::core::multiaddr::Protocol;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::{upgrade, ConnectedPoint, Transport};
use libp2p::request_response::{
    ProtocolName, ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{ConnectionError, DialError, Swarm, SwarmEvent};
use libp2p::{noise, tcp, yamux, Multiaddr};
use libp2p_core::either::EitherOutput;
use libp2p_core::transport::OrTransport;
use libp2p_quic as quic;
use quic::Provider;
use rand::RngCore;
use std::num::NonZeroU8;
use std::time::Duration;
use std::{io, iter};

fn generate_tls_keypair() -> libp2p::identity::Keypair {
    libp2p::identity::Keypair::generate_ed25519()
}

async fn create_swarm<P: Provider>() -> Swarm<RequestResponse<PingCodec>> {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let mut config = quic::Config::new(&keypair);
    config.handshake_timeout = Duration::from_secs(1);
    let transport = quic::GenTransport::<P>::new(config);

    let transport = Transport::map(transport, |(peer, connection), _| {
        (peer, StreamMuxerBox::new(connection))
    })
    .boxed();

    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);
    Swarm::new(transport, behaviour, peer_id)
}

async fn start_listening(swarm: &mut Swarm<RequestResponse<PingCodec>>, addr: &str) -> Multiaddr {
    swarm.listen_on(addr.parse().unwrap()).unwrap();
    match swarm.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    }
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn tokio_smoke() {
    smoke::<quic::tokio::Provider>().await
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn async_std_smoke() {
    smoke::<quic::async_std::Provider>().await
}

async fn smoke<P: Provider>() {
    let _ = env_logger::try_init();
    let mut rng = rand::thread_rng();

    let mut a = create_swarm::<P>().await;
    let mut b = create_swarm::<P>().await;

    let addr = start_listening(&mut a, "/ip4/127.0.0.1/udp/0/quic").await;

    let mut data = vec![0; 4096 * 10];
    rng.fill_bytes(&mut data);

    b.behaviour_mut().add_address(a.local_peer_id(), addr);
    b.behaviour_mut()
        .send_request(a.local_peer_id(), Ping(data.clone()));

    let b_id = *b.local_peer_id();

    let (sync_tx, sync_rx) = oneshot::channel();

    let fut_a = async move {
        match a.next().await {
            Some(SwarmEvent::IncomingConnection { .. }) => {}
            e => panic!("{:?}", e),
        };

        match a.next().await {
            Some(SwarmEvent::ConnectionEstablished { .. }) => {}
            e => panic!("{:?}", e),
        };

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

        a.behaviour_mut()
            .send_request(&b_id, Ping(b"another substream".to_vec()));

        assert!(a.next().now_or_never().is_none());

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

        sync_rx.await.unwrap();

        a.disconnect_peer_id(b_id).unwrap();

        match a.next().await {
            Some(SwarmEvent::ConnectionClosed { cause: None, .. }) => {}
            e => panic!("{:?}", e),
        }
    };

    let fut_b = async {
        match b.next().await {
            Some(SwarmEvent::Dialing(_)) => {}
            e => panic!("{:?}", e),
        }

        match b.next().await {
            Some(SwarmEvent::ConnectionEstablished { .. }) => {}
            e => panic!("{:?}", e),
        };

        assert!(b.next().now_or_never().is_none());

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

        sync_tx.send(()).unwrap();

        match b.next().await {
            Some(SwarmEvent::ConnectionClosed {
                cause: Some(ConnectionError::IO(_)),
                ..
            }) => {}
            e => panic!("{:?}", e),
        }
    };

    join(fut_a, fut_b).await;
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

#[cfg(feature = "async-std")]
#[async_std::test]
async fn dial_failure() {
    let _ = env_logger::try_init();
    let mut a = create_swarm::<quic::async_std::Provider>().await;
    let mut b = create_swarm::<quic::async_std::Provider>().await;

    let addr = start_listening(&mut a, "/ip4/127.0.0.1/udp/0/quic").await;

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
}

#[test]
fn concurrent_connections_and_streams() {
    use quickcheck::*;

    fn prop<P: Provider>(number_listeners: NonZeroU8, number_streams: NonZeroU8) -> TestResult {
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
            let mut listener = pool.run_until(create_swarm::<P>());
            let addr = pool.run_until(start_listening(&mut listener, "/ip4/127.0.0.1/udp/0/quic"));

            listeners.push((*listener.local_peer_id(), addr));

            pool.spawner()
                .spawn_obj(
                    async move {
                        loop {
                            match listener.next().await {
                                Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
                                    message:
                                        RequestResponseMessage::Request {
                                            request: Ping(ping),
                                            channel,
                                            ..
                                        },
                                    ..
                                })) => {
                                    listener
                                        .behaviour_mut()
                                        .send_response(channel, Pong(ping))
                                        .unwrap();
                                }
                                Some(SwarmEvent::Behaviour(
                                    RequestResponseEvent::ResponseSent { .. },
                                ))
                                | Some(SwarmEvent::ConnectionEstablished { .. })
                                | Some(SwarmEvent::IncomingConnection { .. })
                                | Some(SwarmEvent::ConnectionClosed { .. }) => {}
                                Some(e) => {
                                    panic!("unexpected event {:?}", e);
                                }
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

        let mut dialer = pool.run_until(create_swarm::<P>());

        // For each listener node start `number_streams` requests.
        for (listener_peer_id, listener_addr) in &listeners {
            dialer
                .behaviour_mut()
                .add_address(listener_peer_id, listener_addr.clone());

            dialer.dial(*listener_peer_id).unwrap();
        }

        // Wait for responses to each request.
        pool.run_until(async {
            let mut num_responses = 0;
            loop {
                match dialer.next().await {
                    Some(SwarmEvent::Dialing(_)) => {}
                    Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
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
                        num_responses += 1;
                        assert_eq!(data, pong);
                        let should_be = number_listeners as usize * (number_streams) as usize;
                        if num_responses == should_be {
                            break;
                        }
                    }
                    Some(SwarmEvent::ConnectionClosed { .. }) => {}
                    e => {
                        panic!("unexpected event {:?}", e);
                    }
                }
            }
        });

        TestResult::passed()
    }

    #[cfg(feature = "tokio")]
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        QuickCheck::new().quickcheck(prop::<quic::tokio::Provider> as fn(_, _) -> _);
    }

    #[cfg(feature = "async-std")]
    QuickCheck::new().quickcheck(prop::<quic::async_std::Provider> as fn(_, _) -> _);
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn endpoint_reuse() {
    let _ = env_logger::try_init();
    let mut swarm_a = create_swarm::<quic::tokio::Provider>().await;
    let mut swarm_b = create_swarm::<quic::tokio::Provider>().await;
    let b_peer_id = *swarm_b.local_peer_id();

    let a_addr = start_listening(&mut swarm_a, "/ip4/127.0.0.1/udp/0/quic").await;

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

    let b_addr = start_listening(&mut swarm_b, "/ip4/127.0.0.1/udp/0/quic").await;

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
            ev = swarm_b.select_next_some() => {
                if let SwarmEvent::ConnectionEstablished { endpoint, ..} = ev {
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
            },
        }
    }
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn ipv4_dial_ipv6() {
    let _ = env_logger::try_init();
    let mut swarm_a = create_swarm::<quic::async_std::Provider>().await;
    let mut swarm_b = create_swarm::<quic::async_std::Provider>().await;

    let a_addr = start_listening(&mut swarm_a, "/ip6/::1/udp/0/quic").await;

    swarm_b.dial(a_addr.clone()).unwrap();

    loop {
        select! {
            ev = swarm_a.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished { .. } => {
                    return;
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

#[cfg(feature = "async-std")]
#[async_std::test]
async fn wrong_peerid() {
    use libp2p::PeerId;

    let _ = env_logger::try_init();
    let mut swarm_a = create_swarm::<quic::async_std::Provider>().await;
    let mut swarm_b = create_swarm::<quic::async_std::Provider>().await;

    let a_addr = start_listening(&mut swarm_a, "/ip6/::1/udp/0/quic").await;
    let a_id = *swarm_a.local_peer_id();

    let wrong_id = PeerId::random();
    let dial_ops = DialOpts::peer_id(wrong_id).addresses(vec![a_addr]).build();
    swarm_b.dial(dial_ops).unwrap();

    loop {
        select! {
            _ = swarm_a.select_next_some() => {},
            ev = swarm_b.select_next_some() => match ev {
                SwarmEvent::Dialing(peer_id) => assert_eq!(peer_id, wrong_id),
                SwarmEvent::OutgoingConnectionError {peer_id: Some(peer_id), error: DialError::WrongPeerId { obtained, .. }} => {
                    assert_eq!(peer_id, wrong_id);
                    assert_eq!(obtained, a_id);
                    break;
                },
                e => panic!("{:?}", e),
            }
        }
    }
}

#[cfg(feature = "async-std")]
fn new_tcp_quic_swarm() -> Swarm<RequestResponse<PingCodec>> {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let mut config = quic::Config::new(&keypair);
    config.handshake_timeout = Duration::from_secs(1);
    let quic_transport = quic::async_std::Transport::new(config);
    let tcp_transport = tcp::async_io::Transport::new(tcp::Config::default())
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(
            noise::NoiseConfig::xx(
                noise::Keypair::<noise::X25519Spec>::new()
                    .into_authentic(&keypair)
                    .unwrap(),
            )
            .into_authenticated(),
        )
        .multiplex(yamux::YamuxConfig::default());

    let transport = OrTransport::new(quic_transport, tcp_transport)
        .map(|either_output, _| match either_output {
            EitherOutput::First((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            EitherOutput::Second((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed();

    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);

    Swarm::new(transport, behaviour, peer_id)
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn tcp_and_quic() {
    let mut swarm_a = new_tcp_quic_swarm();
    let swarm_a_id = *swarm_a.local_peer_id();
    println!("{}", swarm_a_id);

    let mut swarm_b = new_tcp_quic_swarm();

    let quic_addr = start_listening(&mut swarm_a, "/ip4/127.0.0.1/udp/0/quic").await;
    let tcp_addr = start_listening(&mut swarm_a, "/ip4/127.0.0.1/tcp/0").await;

    swarm_b.dial(quic_addr.clone()).unwrap();

    loop {
        select! {
                ev = swarm_a.select_next_some() => match ev {
                    SwarmEvent::ConnectionEstablished { .. } => break,
                    SwarmEvent::IncomingConnection { .. } => { }
                    e => panic!("{:?}", e),
                },
                ev = swarm_b.select_next_some() => match ev {
                    SwarmEvent::ConnectionEstablished { .. } => {},
                    e => panic!("{:?}", e),
            }
        }
    }

    swarm_b.dial(tcp_addr).unwrap();

    loop {
        select! {
                ev = swarm_a.select_next_some() => match ev {
                    SwarmEvent::ConnectionEstablished { .. } => break,
                    SwarmEvent::IncomingConnection { .. }
                    | SwarmEvent::ConnectionClosed { .. } => { }
                    e => panic!("{:?}", e),
                },
                ev = swarm_b.select_next_some() => match ev {
                    SwarmEvent::ConnectionEstablished { .. } => {},
                    SwarmEvent::ConnectionClosed { endpoint, .. } => {
                        assert_eq!(endpoint.get_remote_address(), &quic_addr );
                    }
                    e => panic!("{:?}", e),
            }
        }
    }
}
