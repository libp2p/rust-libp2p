use async_std::task::sleep;
use asynchronous_codec::{CborCodec, Framed};
use common::{mk_transport, setup_logger};
use futures::{channel::mpsc, SinkExt, StreamExt};
use libp2p_streaming::{Streaming, StreamingCodec, StreamingConfig, StreamingEvent};
use libp2p_swarm::{NegotiatedSubstream, Swarm, SwarmEvent};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

mod common;

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct RequestTicker;
#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct ResponseTicker(Duration);

#[derive(Debug)]
struct RequestCodec;
impl StreamingCodec for RequestCodec {
    type Protocol = &'static [u8];

    type Upgrade = Framed<NegotiatedSubstream, CborCodec<RequestTicker, ResponseTicker>>;

    fn upgrade(io: NegotiatedSubstream) -> Self::Upgrade {
        Framed::new(io, CborCodec::<RequestTicker, ResponseTicker>::new())
    }

    fn protocol_name() -> Self::Protocol {
        b"/streaming/ticker/1.0.0"
    }
}
#[derive(Debug)]
struct ResponseCodec;
impl StreamingCodec for ResponseCodec {
    type Protocol = &'static [u8];

    type Upgrade = Framed<NegotiatedSubstream, CborCodec<ResponseTicker, RequestTicker>>;

    fn upgrade(io: NegotiatedSubstream) -> Self::Upgrade {
        Framed::new(io, CborCodec::<ResponseTicker, RequestTicker>::new())
    }

    fn protocol_name() -> Self::Protocol {
        b"/streaming/ticker/1.0.0"
    }
}

#[async_std::test]
async fn ticker() -> anyhow::Result<()> {
    setup_logger();

    let (peer1_id, trans) = mk_transport();

    let config = StreamingConfig::new(Duration::from_millis(50), Duration::from_millis(100));
    let mut swarm1 = Swarm::new(
        trans,
        Streaming::<RequestCodec>::new(config.clone()),
        peer1_id,
    );

    let (peer2_id, trans) = mk_transport();
    let mut swarm2 = Swarm::new(trans, Streaming::<ResponseCodec>::new(config), peer2_id);

    let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    swarm1.listen_on(addr).unwrap();

    let (mut tx_addr, mut rx_addr) = mpsc::channel(1);
    let (tx, mut rx) = mpsc::channel(1);
    let swarm1_handle = async_std::task::spawn(async move {
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
                    stream.send(RequestTicker).await.unwrap();

                    let mut txc = tx.clone();
                    async_std::task::spawn(async move {
                        let mut i = 0usize;
                        while let Some(Ok(x)) = stream.next().await {
                            tracing::info!("{}", x.0.as_millis());
                            i += 1;
                        }
                        txc.send(i).await.unwrap();
                    });
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    assert_eq!(peer_id, peer2_id);
                }
                SwarmEvent::Behaviour(StreamingEvent::InboundFailure { peer_id, .. }) => {
                    assert_eq!(peer_id, peer2_id);
                    break;
                }
                _ => {}
            }
        }
    });

    let addr = rx_addr.next().await.unwrap();
    swarm2.behaviour_mut().add_address(peer1_id, addr.clone());

    let stream_id = swarm2.behaviour_mut().open_stream(peer1_id);
    while let Some(ev) = swarm2.next().await {
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
                assert_eq!(out, RequestTicker);

                async_std::task::spawn(async move {
                    let start = Instant::now();
                    for _ in 0..10 {
                        stream.send(ResponseTicker(start.elapsed())).await.unwrap();
                        sleep(Duration::from_millis(50)).await;
                    }
                    stream.close().await.unwrap();
                });
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                assert_eq!(peer_id, peer1_id);
                assert_eq!(rx.next().await.unwrap(), 10);
                break;
            }

            _ => {}
        }
    }
    swarm1_handle.await;

    // TODO: assert streamclosed message
    Ok(())
}
