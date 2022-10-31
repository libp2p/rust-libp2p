// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use anyhow::Result;
use async_trait::async_trait;
use futures::{
    future::{select, Either, FutureExt},
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    stream::StreamExt,
};
use libp2p::core::{identity, muxing::StreamMuxerBox, upgrade, Transport as _};
use libp2p::request_response::{
    ProtocolName, ProtocolSupport, RequestResponse, RequestResponseCodec, RequestResponseConfig,
    RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::{Swarm, SwarmBuilder, SwarmEvent};
use libp2p::webrtc::tokio as webrtc;
use rand::RngCore;

use std::{io, iter};

#[tokio::test]
async fn smoke() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut rng = rand::thread_rng();

    let mut a = create_swarm()?;
    let mut b = create_swarm()?;

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;
    Swarm::listen_on(&mut b, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;

    let addr = match a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    // skip other interface addresses
    while a.next().now_or_never().is_some() {}

    let _ = match b.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    // skip other interface addresses
    while b.next().now_or_never().is_some() {}

    let mut data = vec![0; 4096];
    rng.fill_bytes(&mut data);

    b.behaviour_mut()
        .add_address(Swarm::local_peer_id(&a), addr);
    b.behaviour_mut()
        .send_request(Swarm::local_peer_id(&a), Ping(data.clone()));

    match b.next().await {
        Some(SwarmEvent::Dialing(_)) => {}
        e => panic!("{:?}", e),
    }

    let pair = select(a.next(), b.next());
    match pair.await {
        Either::Left((Some(SwarmEvent::IncomingConnection { .. }), _)) => {}
        Either::Left((e, _)) => panic!("{:?}", e),
        Either::Right(_) => panic!("b completed first"),
    }

    let pair = select(a.next(), b.next());
    match pair.await {
        Either::Left((Some(SwarmEvent::ConnectionEstablished { .. }), _)) => {}
        Either::Left((e, _)) => panic!("{:?}", e),
        Either::Right((Some(SwarmEvent::ConnectionEstablished { .. }), _)) => {}
        Either::Right((e, _)) => panic!("{:?}", e),
    }

    let pair = select(a.next(), b.next());
    match pair.await {
        Either::Left((Some(SwarmEvent::ConnectionEstablished { .. }), _)) => {}
        Either::Left((e, _)) => panic!("{:?}", e),
        Either::Right((Some(SwarmEvent::ConnectionEstablished { .. }), _)) => {}
        Either::Right((e, _)) => panic!("{:?}", e),
    }

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
        Swarm::local_peer_id(&b),
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

#[tokio::test]
async fn dial_failure() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut a = create_swarm()?;
    let mut b = create_swarm()?;

    Swarm::listen_on(&mut a, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;
    Swarm::listen_on(&mut b, "/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;

    let addr = match a.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    // skip other interface addresses
    while a.next().now_or_never().is_some() {}

    let _ = match b.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    // skip other interface addresses
    while b.next().now_or_never().is_some() {}

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

    let num_listeners = 3usize;
    let num_streams = 8usize;

    let mut data = vec![0; 4096];
    rand::thread_rng().fill_bytes(&mut data);
    let mut listeners = vec![];

    // Spawn the listener nodes.
    for _ in 0..num_listeners {
        let mut listener = create_swarm().unwrap();
        Swarm::listen_on(
            &mut listener,
            "/ip4/127.0.0.1/udp/0/webrtc".parse().unwrap(),
        )
        .unwrap();

        // Wait to listen on address.
        let addr = match listener.next().await {
            Some(SwarmEvent::NewListenAddr { address, .. }) => address,
            e => panic!("{:?}", e),
        };

        listeners.push((*listener.local_peer_id(), addr));

        tokio::spawn(async move {
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
                    Some(SwarmEvent::Behaviour(RequestResponseEvent::ResponseSent { .. })) => {
                        log::debug!("listener ResponseSent");
                    }
                    Some(SwarmEvent::ConnectionClosed { .. }) => {}
                    Some(SwarmEvent::NewListenAddr { .. }) => {
                        log::debug!("listener NewListenAddr");
                    }
                    Some(e) => {
                        panic!("unexpected event {:?}", e);
                    }
                    None => {
                        panic!("listener stopped");
                    }
                }
            }
        });
    }

    let mut dialer = create_swarm().unwrap();
    Swarm::listen_on(&mut dialer, "/ip4/127.0.0.1/udp/0/webrtc".parse().unwrap()).unwrap();

    // Wait to listen on address.
    match dialer.next().await {
        Some(SwarmEvent::NewListenAddr { address, .. }) => address,
        e => panic!("{:?}", e),
    };

    // For each listener node start `number_streams` requests.
    for (listener_peer_id, listener_addr) in &listeners {
        dialer
            .behaviour_mut()
            .add_address(listener_peer_id, listener_addr.clone());

        dialer.dial(*listener_peer_id).unwrap();
    }

    // Wait for responses to each request.
    let mut num_responses = 0;
    loop {
        match dialer.next().await {
            Some(SwarmEvent::Dialing(_)) => {
                log::debug!("dialer Dialing");
            }
            Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                log::debug!("dialer Connection established");
                for _ in 0..num_streams {
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
                let should_be = num_listeners * num_streams;
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
            Some(SwarmEvent::NewListenAddr { .. }) => {
                log::debug!("dialer NewListenAddr");
            }
            e => {
                panic!("unexpected event {:?}", e);
            }
        }
    }
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

fn create_swarm() -> Result<Swarm<RequestResponse<PingCodec>>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().to_peer_id();
    let transport = webrtc::Transport::new(id_keys)?;

    let protocols = iter::once((PingProtocol(), ProtocolSupport::Full));
    let cfg = RequestResponseConfig::default();
    let behaviour = RequestResponse::new(PingCodec(), protocols, cfg);
    let transport = transport
        .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
        .boxed();

    Ok(SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(|fut| {
            tokio::spawn(fut);
        }))
        .build())
}
