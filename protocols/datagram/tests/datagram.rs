use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt as _;
use libp2p_core::{Transport as _, muxing::StreamMuxerBox};
use libp2p_datagram as datagram;
use libp2p_identity::Keypair;
use libp2p_swarm::{Config, NetworkBehaviour, StreamProtocol, Swarm, SwarmEvent};

fn quic_swarm() -> Swarm<datagram::Behaviour> {
    let keypair = Keypair::generate_ed25519();
    let peer_id = keypair.public().to_peer_id();
    let transport = libp2p_quic::tokio::Transport::new(libp2p_quic::Config::new(&keypair))
        .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        .boxed();
    Swarm::new(
        transport,
        datagram::Behaviour::new(StreamProtocol::new("/example/datagram/1.0.0")),
        peer_id,
        Config::with_tokio_executor().with_idle_connection_timeout(Duration::from_secs(10)),
    )
}

#[tokio::test]
async fn datagram_roundtrip_over_quic() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let mut listener = quic_swarm();
    let mut dialer = quic_swarm();

    let listener_peer = *listener.local_peer_id();
    let dialer_peer = *dialer.local_peer_id();

    let mut incoming = listener.behaviour_mut().incoming_datagrams().unwrap();
    let mut control = dialer.behaviour().new_control();

    listener
        .listen_on("/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap())
        .unwrap();
    let addr = loop {
        if let SwarmEvent::NewListenAddr { address, .. } = listener.select_next_some().await {
            break address;
        }
    };

    dialer.dial(addr).unwrap();

    let payload = Bytes::from_static(b"unreliable datagram");
    // Lossy: resend until one lands.
    let mut resend = tokio::time::interval(Duration::from_millis(20));
    let mut connected = false;
    let mut conn_id = None;

    let (from, data) = loop {
        tokio::select! {
            _ = listener.select_next_some() => {}
            event = dialer.select_next_some() => {
                if let SwarmEvent::ConnectionEstablished { connection_id, .. } = event {
                    conn_id = Some(connection_id);
                    connected = true;
                }
            }
            _ = resend.tick(), if connected => {
                let _ = control.send_datagram(listener_peer, payload.clone());
            }
            Some(msg) = incoming.next() => break msg,
        }
    };

    assert_eq!(from, dialer_peer);
    assert_eq!(data, payload);

    // The connection's max datagram size is learned from the send path.
    let conn_id = conn_id.unwrap();
    let max = loop {
        if let Some(max) = control.max_datagram_size(listener_peer, conn_id) {
            break max;
        }
        tokio::select! {
            _ = listener.select_next_some() => {}
            _ = dialer.select_next_some() => {}
            _ = resend.tick() => {
                let _ = control.send_datagram(listener_peer, payload.clone());
            }
        }
    };
    assert!(max > 0);
}

/// Two datagram behaviours on one connection, combined through
/// `#[derive(NetworkBehaviour)]` (i.e. a nested `ConnectionHandlerSelect`, as a
/// real node composes them). A datagram sent on one must route only to that
/// behaviour, keyed on its control-stream id.
#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct TwoDatagrams {
    a: datagram::Behaviour,
    b: datagram::Behaviour,
}

fn composed_quic_swarm() -> Swarm<TwoDatagrams> {
    let keypair = Keypair::generate_ed25519();
    let peer_id = keypair.public().to_peer_id();
    let transport = libp2p_quic::tokio::Transport::new(libp2p_quic::Config::new(&keypair))
        .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        .boxed();
    Swarm::new(
        transport,
        TwoDatagrams {
            a: datagram::Behaviour::new(StreamProtocol::new("/example/dg-a/1.0.0")),
            b: datagram::Behaviour::new(StreamProtocol::new("/example/dg-b/1.0.0")),
        },
        peer_id,
        Config::with_tokio_executor().with_idle_connection_timeout(Duration::from_secs(10)),
    )
}

#[tokio::test]
async fn datagram_routes_to_owning_behaviour_when_composed() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let mut listener = composed_quic_swarm();
    let mut dialer = composed_quic_swarm();

    let listener_peer = *listener.local_peer_id();
    let dialer_peer = *dialer.local_peer_id();

    let mut a_in = listener.behaviour_mut().a.incoming_datagrams().unwrap();
    let mut b_in = listener.behaviour_mut().b.incoming_datagrams().unwrap();
    let mut a_control = dialer.behaviour().a.new_control();

    listener
        .listen_on("/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap())
        .unwrap();
    let addr = loop {
        if let SwarmEvent::NewListenAddr { address, .. } = listener.select_next_some().await {
            break address;
        }
    };

    dialer.dial(addr).unwrap();

    let payload = Bytes::from_static(b"for a only");
    let mut resend = tokio::time::interval(Duration::from_millis(20));
    let mut connected = false;

    let deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(deadline);

    // Sent on `a`, so it must surface on `a` and never leak into `b`.
    let (from, data) = loop {
        tokio::select! {
            _ = &mut deadline => panic!("timed out waiting for the datagram on `a`"),
            _ = listener.select_next_some() => {}
            event = dialer.select_next_some() => {
                if let SwarmEvent::ConnectionEstablished { .. } = event {
                    connected = true;
                }
            }
            _ = resend.tick(), if connected => {
                let _ = a_control.send_datagram(listener_peer, payload.clone());
            }
            Some(msg) = a_in.next() => break msg,
            Some(_) = b_in.next() => panic!("datagram for `a` leaked into `b`"),
        }
    };

    assert_eq!(from, dialer_peer);
    assert_eq!(data, payload);
}
