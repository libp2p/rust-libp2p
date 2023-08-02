use anyhow::Result;
use axum::{http::Method, routing::get, Router};
use futures::StreamExt;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::Transport;
use libp2p_identity as identity;
use libp2p_ping as ping;
use libp2p_relay as relay;
use libp2p_swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent};
use libp2p_webrtc as webrtc;
use multiaddr::{Multiaddr, Protocol};
use rand::thread_rng;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tower_http::cors::{Any, CorsLayer};
use void::Void;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let id_keys = identity::Keypair::generate_ed25519();
    let local_peer_id = id_keys.public().to_peer_id();
    let transport = webrtc::tokio::Transport::new(
        id_keys,
        webrtc::tokio::Certificate::generate(&mut thread_rng())?,
    )
    .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
    .boxed();

    let behaviour = Behaviour {
        relay: relay::Behaviour::new(local_peer_id, Default::default()),
        ping: ping::Behaviour::new(ping::Config::new()),
        keep_alive: keep_alive::Behaviour,
    };

    let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build();

    let address_webrtc = Multiaddr::from(Ipv6Addr::LOCALHOST)
        .with(Protocol::Udp(0))
        .with(Protocol::WebRTCDirect);

    swarm.listen_on(address_webrtc.clone())?;

    // Dial the peer identified by the multi-address given as the second
    // command-line argument, if any.
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        println!("Dialed {addr}")
    }

    loop {
        tokio::select! {
            evt = swarm.select_next_some() => {
                match evt {
                    SwarmEvent::NewListenAddr { address, .. } => {

                        let addr = address
                                .with(Protocol::P2p(*swarm.local_peer_id()))
                                .clone()
                                .to_string();

                        eprintln!("Listening on {}", addr);

                        tokio::spawn(async move {

                            let app = Router::new().route("/", get(|| async { addr }))
                            .layer(
                                // allow cors
                                CorsLayer::new()
                                    .allow_origin(Any)
                                    .allow_methods([Method::GET]),
                             );

                            axum::Server::bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), webrtc_websys_tests::PORT))
                                .serve(app.into_make_service())
                                .await
                                .unwrap();
                        });

                        eprintln!("Server spawned");
                    }
                    SwarmEvent::Behaviour(Event::Ping(ping::Event {
                        peer,
                        result: Ok(rtt),
                        ..
                    })) => {
                        let id = peer.to_string().to_owned();
                        eprintln!("🏓 Pinged {id} ({rtt:?})")
                    }
                    evt => {
                        eprintln!("SwarmEvent: {:?}", evt);
                    },
                }
            },
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }
    Ok(())
}

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "Event", prelude = "libp2p_swarm::derive_prelude")]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
    relay: relay::Behaviour,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum Event {
    Ping(ping::Event),
    Relay(relay::Event),
}

impl From<ping::Event> for Event {
    fn from(event: ping::Event) -> Self {
        Event::Ping(event)
    }
}

impl From<Void> for Event {
    fn from(event: Void) -> Self {
        void::unreachable(event)
    }
}

impl From<relay::Event> for Event {
    fn from(event: relay::Event) -> Self {
        Event::Relay(event)
    }
}
