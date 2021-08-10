use async_std::prelude::*;
use common::{mk_transport, setup_logger};
use futures::{channel::mpsc, SinkExt, StreamExt};
use libp2p_streaming::{IdentityCodec, Streaming};
use libp2p_swarm::{Swarm, SwarmEvent};

mod common;

#[async_std::test]
async fn smoke() -> anyhow::Result<()> {
    setup_logger();

    let (peer1_id, trans) = mk_transport();

    let mut swarm1 = Swarm::new(trans, Streaming::<IdentityCodec>::default(), peer1_id);

    let (peer2_id, trans) = mk_transport();
    let mut swarm2 = Swarm::new(trans, Streaming::<IdentityCodec>::default(), peer2_id);

    let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    swarm1.listen_on(addr).unwrap();

    let (mut tx_addr, mut rx_addr) = mpsc::channel(1);
    async_std::task::spawn(async move {
        while let Some(ev) = swarm1.next().await {
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
                    stream.write_all(b"Hello").await.unwrap();
                    stream.flush().await.unwrap();

                    let mut out = vec![0; 32];
                    let n = stream.read(&mut out).await.unwrap();
                    out.truncate(n);
                    assert_eq!(out, b"World!");
                    break;
                }
                x => {
                    tracing::info!("swarm1: {:?}", x);
                }
            };
        }
    });

    let addr = rx_addr.next().await.unwrap();
    swarm2.behaviour_mut().add_address(peer1_id, addr.clone());
    let stream_id = swarm2.behaviour_mut().open_stream(peer1_id);

    while let Some(ev) = swarm2.next().await {
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
                let mut out = vec![0; 32];
                let n = stream.read(&mut out).await.unwrap();
                out.truncate(n);
                assert_eq!(out, b"Hello");

                stream.write_all(b"World!").await.unwrap();
                stream.flush().await.unwrap();
                break;
            }
            x => {
                tracing::info!("swarm2: {:?}", x);
            }
        }
    }

    Ok(())
}
