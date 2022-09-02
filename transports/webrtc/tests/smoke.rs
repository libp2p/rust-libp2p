use anyhow::Result;
use async_trait::async_trait;
use futures::{
    future::{join, select, Either, FutureExt},
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    stream::StreamExt,
};
use libp2p::request_response::{
    ProtocolName, ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::{Swarm, SwarmBuilder, SwarmEvent};
use libp2p_core::{identity, multiaddr::Protocol, muxing::StreamMuxerBox, upgrade, Transport};
use libp2p_webrtc::transport::WebRTCTransport;
use multihash::{Code, Multihash, MultihashDigest};
use rand::RngCore;
use rcgen::KeyPair;
use tokio_crate as tokio;
use webrtc::peer_connection::certificate::RTCCertificate;

use std::{io, iter};

fn generate_certificate() -> RTCCertificate {
    let kp = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key pair");
    RTCCertificate::from_key_pair(kp).expect("certificate")
}

fn generate_tls_keypair() -> identity::Keypair {
    identity::Keypair::generate_ed25519()
}

async fn create_swarm() -> Result<(Swarm<RequestResponse<PingCodec>>, String)> {
    let cert = generate_certificate();
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let fingerprint = cert
        .get_fingerprints()
        .unwrap()
        .first()
        .unwrap()
        .value
        .to_uppercase();
    let transport = WebRTCTransport::new(cert, keypair);
    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);
    let transport = Transport::map(transport, |(peer_id, conn), _| {
        (peer_id, StreamMuxerBox::new(conn))
    })
    .boxed();
    Ok((
        SwarmBuilder::new(transport, behaviour, peer_id)
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build(),
        fingerprint,
    ))
}

#[tokio::test]
async fn smoke() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut rng = rand::thread_rng();

    let (mut a, a_fingerprint) = create_swarm().await?;
    let (mut b, _b_fingerprint) = create_swarm().await?;

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;
    Swarm::listen_on(&mut b, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;

    let addr = match a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    let addr = addr.with(Protocol::Certhash(fingerprint2multihash(&a_fingerprint)));

    let _ = match b.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    let mut data = vec![0; 4096];
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

    let pair = join(a.next(), b.next());
    match pair.await {
        (
            Some(SwarmEvent::ConnectionEstablished { .. }),
            Some(SwarmEvent::ConnectionEstablished { .. }),
        ) => {}
        e => panic!("{:?}", e),
    };

    assert!(b.next().now_or_never().is_none());

    let pair = select(a.next(), b.next());
    match pair.await {
        Either::Left((
            Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
                message:
                    RequestResponseMessage::Request {
                        request: Ping(ping),
                        channel,
                        ..
                    },
                ..
            })),
            _,
        )) => {
            a.behaviour_mut()
                .send_response(channel, Pong(ping))
                .unwrap();
        }
        Either::Left((e, _)) => panic!("{:?}", e),
        Either::Right(_) => panic!("b completed first"),
    }

    match a.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::ResponseSent { .. })) => {}
        e => panic!("{:?}", e),
    }

    let pair = select(a.next(), b.next());
    match pair.await {
        Either::Right((
            Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
                message:
                    RequestResponseMessage::Response {
                        response: Pong(pong),
                        ..
                    },
                ..
            })),
            _,
        )) => assert_eq!(data, pong),
        Either::Right((e, _)) => panic!("{:?}", e),
        Either::Left(_) => panic!("a completed first"),
    }

    a.behaviour_mut().send_request(
        &Swarm::local_peer_id(&b),
        Ping(b"another substream".to_vec()),
    );

    assert!(a.next().now_or_never().is_none());

    let pair = select(a.next(), b.next());
    match pair.await {
        Either::Right((
            Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
                message:
                    RequestResponseMessage::Request {
                        request: Ping(data),
                        channel,
                        ..
                    },
                ..
            })),
            _,
        )) => {
            b.behaviour_mut()
                .send_response(channel, Pong(data))
                .unwrap();
        }
        Either::Right((e, _)) => panic!("{:?}", e),
        Either::Left(_) => panic!("a completed first"),
    }

    match b.next().await {
        Some(SwarmEvent::Behaviour(RequestResponseEvent::ResponseSent { .. })) => {}
        e => panic!("{:?}", e),
    }

    let pair = select(a.next(), b.next());
    match pair.await {
        Either::Left((
            Some(SwarmEvent::Behaviour(RequestResponseEvent::Message {
                message:
                    RequestResponseMessage::Response {
                        response: Pong(data),
                        ..
                    },
                ..
            })),
            _,
        )) => assert_eq!(data, b"another substream".to_vec()),
        Either::Left((e, _)) => panic!("{:?}", e),
        Either::Right(_) => panic!("b completed first"),
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
        upgrade::read_length_prefixed(io, 4096)
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
        upgrade::read_length_prefixed(io, 4096)
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
    let _ = env_logger::builder().is_test(true).try_init();

    let (mut a, a_fingerprint) = create_swarm().await?;
    let (mut b, _b_fingerprint) = create_swarm().await?;

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;
    Swarm::listen_on(&mut b, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;

    let addr = match a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    let addr = addr.with(Protocol::Certhash(fingerprint2multihash(&a_fingerprint)));

    let _ = match b.next().await {
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
    let _ = env_logger::builder().is_test(true).try_init();

    use futures::executor::block_on;
    use futures::task::Spawn;
    use quickcheck::*;
    use std::num::NonZeroU8;

    fn prop(number_listeners: NonZeroU8, number_streams: NonZeroU8) -> TestResult {
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
            let (mut listener, fingerprint) = block_on(create_swarm()).unwrap();
            Swarm::listen_on(
                &mut listener,
                "/ip4/127.0.0.1/udp/0/webrtc".parse().unwrap(),
            )
            .unwrap();

            // Wait to listen on address.
            let addr = match block_on(listener.next()) {
                Some(SwarmEvent::NewListenAddr { address, .. }) => address,
                e => panic!("{:?}", e),
            };

            let addr = addr.with(Protocol::Certhash(fingerprint2multihash(&fingerprint)));

            listeners.push((*listener.local_peer_id(), addr));

            pool.spawner()
                .spawn_obj(
                    async move {
                        loop {
                            match listener.next().await {
                                Some(SwarmEvent::IncomingConnection { .. }) => {
                                    log::debug!("listener IncomingConnection");
                                }
                                Some(SwarmEvent::ConnectionEstablished { .. }) => {
                                    log::debug!("listener ConnectionEstablished");
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
                                    log::debug!("listener got Message");
                                    listener
                                        .behaviour_mut()
                                        .send_response(channel, Pong(ping))
                                        .unwrap();
                                }
                                Some(SwarmEvent::Behaviour(
                                    RequestResponseEvent::ResponseSent { .. },
                                )) => {
                                    log::debug!("listener ResponseSent");
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
                    .boxed()
                    .into(),
                )
                .unwrap();
        }

        let (mut dialer, _fingerprint) = block_on(create_swarm()).unwrap();
        Swarm::listen_on(&mut dialer, "/ip4/127.0.0.1/udp/0/webrtc".parse().unwrap()).unwrap();

        // Wait to listen on address.
        match block_on(dialer.next()) {
            Some(SwarmEvent::NewListenAddr { address, .. }) => address,
            e => panic!("{:?}", e),
        };

        // For each listener node start `number_streams` requests.
        for (listener_peer_id, listener_addr) in &listeners {
            dialer
                .behaviour_mut()
                .add_address(&listener_peer_id, listener_addr.clone());

            dialer.dial(listener_peer_id.clone()).unwrap();
        }

        // Wait for responses to each request.
        pool.run_until(async {
            let mut num_responses = 0;
            loop {
                match dialer.next().await {
                    Some(SwarmEvent::Dialing(_)) => {
                        log::debug!("dialer Dialing");
                    }
                    Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                        log::debug!("dialer Connection established");
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
                        log::debug!("dialer got Message");
                        num_responses += 1;
                        assert_eq!(data, pong);
                        let should_be = number_listeners as usize * (number_streams) as usize;
                        log::debug!(
                            "num of responses: {}, num of listeners * num of streams: {}",
                            num_responses,
                            should_be
                        );
                        if num_responses == should_be {
                            break;
                        }
                    }
                    Some(SwarmEvent::ConnectionClosed { .. }) => {
                        log::debug!("dialer ConnectionClosed");
                    }
                    e => {
                        panic!("unexpected event {:?}", e);
                    }
                }
            }
        });

        TestResult::passed()
    }

    prop(NonZeroU8::new(3).unwrap(), NonZeroU8::new(8).unwrap());

    // QuickCheck::new().quickcheck(prop as fn(_, _) -> _);
}

fn fingerprint2multihash(s: &str) -> Multihash {
    let mut buf = [0; 32];
    hex::decode_to_slice(s.replace(":", ""), &mut buf).unwrap();
    Code::Sha2_256.wrap(&buf).unwrap()
}
