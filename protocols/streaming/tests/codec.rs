use async_std::future::timeout;
use asynchronous_codec::{CborCodec, Framed};
use common::{mk_transport, setup_logger};
use futures::{channel::mpsc, SinkExt, StreamExt};
use libp2p_streaming::{Streaming, StreamingCodec};
use libp2p_swarm::{NegotiatedSubstream, Swarm, SwarmEvent};
use serde::{Deserialize, Serialize};
///! Simple example demonstrating a custom codec.
use std::time::Duration;

mod common;

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct Ping(Vec<u8>);
#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct Pong(Vec<u8>);
#[derive(Debug)]
struct PingCodec;
impl StreamingCodec for PingCodec {
    type Protocol = &'static [u8];

    type Upgrade = Framed<NegotiatedSubstream, CborCodec<Ping, Pong>>;

    fn upgrade(io: NegotiatedSubstream) -> Self::Upgrade {
        Framed::new(io, CborCodec::new())
    }

    fn protocol_name() -> Self::Protocol {
        b"/streaming/ping/1.0.0"
    }
}

#[derive(Debug)]
struct PongCodec;
impl StreamingCodec for PongCodec {
    type Protocol = &'static [u8];

    type Upgrade = Framed<NegotiatedSubstream, CborCodec<Pong, Ping>>;

    fn upgrade(io: NegotiatedSubstream) -> Self::Upgrade {
        Framed::new(io, CborCodec::new())
    }

    fn protocol_name() -> Self::Protocol {
        b"/streaming/ping/1.0.0"
    }
}

#[async_std::test]
async fn codec() -> anyhow::Result<()> {
    setup_logger();

    let (peer1_id, trans) = mk_transport();

    let mut swarm1 = Swarm::new(trans, Streaming::<PingCodec>::default(), peer1_id);

    let (peer2_id, trans) = mk_transport();
    let mut swarm2 = Swarm::new(trans, Streaming::<PongCodec>::default(), peer2_id);

    let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    swarm1.listen_on(addr).unwrap();

    let (mut tx_addr, mut rx_addr) = mpsc::channel(1);
    async_std::task::spawn(async move {
        while let Some(ev) = swarm1.next().await {
            tracing::info!("swarm1: {:?}", ev);
            match ev {
                SwarmEvent::NewListenAddr { address, .. } => {
                    tx_addr.send(address).await.unwrap();
                }
                SwarmEvent::ListenerError { error, .. } => panic!("{}", error),
                SwarmEvent::Behaviour(libp2p_streaming::StreamingEvent::NewIncoming {
                    peer_id,
                    mut stream,
                    ..
                }) => {
                    assert_eq!(peer_id, peer2_id);
                    stream.send(Ping(b"Hello".to_vec())).await.unwrap();

                    let out = stream.next().await.unwrap().unwrap();
                    assert_eq!(out, Pong(b"World!".to_vec()));
                    break;
                }
                _ => {}
            }
        }
    });

    let addr = rx_addr.next().await.unwrap();
    swarm2.behaviour_mut().add_address(peer1_id, addr.clone());

    let stream_id = swarm2.behaviour_mut().open_stream(peer1_id);
    while let Some(ev) = timeout(Duration::from_secs(5), swarm2.next())
        .await
        .unwrap()
    {
        tracing::info!("swarm2: {:?}", ev);
        match ev {
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                assert_eq!(peer1_id, peer_id);
            }
            SwarmEvent::Behaviour(libp2p_streaming::StreamingEvent::StreamOpened {
                id,
                peer_id,
                mut stream,
            }) => {
                assert_eq!(peer1_id, peer_id);
                assert_eq!(id, stream_id);
                let out = stream.next().await.unwrap().unwrap();
                assert_eq!(out, Ping(b"Hello".to_vec()));

                stream.send(Pong(b"World".to_vec())).await.unwrap();
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
