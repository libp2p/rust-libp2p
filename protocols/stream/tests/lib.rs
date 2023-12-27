use futures::{AsyncReadExt as _, AsyncWriteExt as _, StreamExt as _};
use libp2p_stream as stream;
use libp2p_swarm::{StreamProtocol, Swarm};
use libp2p_swarm_test::SwarmExt as _;
use stream::OpenStreamError;

const PROTOCOL: StreamProtocol = StreamProtocol::new("/test");

#[tokio::test]
async fn dropping_incoming_streams_deregisters() {
    let mut swarm1 = Swarm::new_ephemeral(|_| stream::Behaviour::default());
    let mut swarm2 = Swarm::new_ephemeral(|_| stream::Behaviour::default());

    let mut control = swarm1.behaviour_mut().new_control(PROTOCOL);
    let mut incoming = swarm2.behaviour_mut().accept(PROTOCOL).unwrap();

    swarm2.listen().with_memory_addr_external().await;
    swarm1.connect(&mut swarm2).await;

    let swarm2_peer_id = *swarm2.local_peer_id();

    let handle = tokio::spawn(async move {
        while let Some((_, mut stream)) = incoming.next().await {
            stream.write_all(&[42]).await.unwrap();
            stream.close().await.unwrap();
        }
    });
    tokio::spawn(swarm1.loop_on_next());
    tokio::spawn(swarm2.loop_on_next());

    let mut peer_control = control.peer(swarm2_peer_id).await.unwrap();
    let mut stream = peer_control.open_stream().await.unwrap();

    let mut buf = [0u8; 1];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!([42], buf);

    handle.abort();

    let error = peer_control.open_stream().await.unwrap_err();
    assert!(matches!(error, OpenStreamError::UnsupportedProtocol));
}
