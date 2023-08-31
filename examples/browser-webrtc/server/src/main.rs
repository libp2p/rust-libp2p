use anyhow::Result;
use axum::{
    body::{boxed, Body, BoxBody},
    http::Method,
    http::{Request, Response, Uri},
    routing::get,
    Router,
};
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::Transport;
use libp2p::identity;
use libp2p::ping;
use libp2p::relay;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent};
use libp2p_webrtc as webrtc;
use multiaddr::{Multiaddr, Protocol};
use rand::thread_rng;
use std::net::Ipv6Addr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .parse_filters("webrtc_example_server=debug,libp2p_webrtc=info,libp2p_ping=debug")
        .parse_default_env()
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

    loop {
        if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
            let addr = address
                .with(Protocol::P2p(*swarm.local_peer_id()))
                .clone()
                .to_string();

            log::info!("Listening on: {}", addr);

            // Serve the multiaddress over HTTP
            tokio::spawn(serve(addr));

            break;
        }
    }

    loop {
        tokio::select! {
            swarm_event = swarm.next() => {
                log::trace!("Swarm Event: {:?}", swarm_event)
            },
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    Ok(())
}

const PORT: u16 = 8080;

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
    relay: relay::Behaviour,
}

/// Serve the Multiaddr we are listening on and the host files.
pub(crate) async fn serve(address: String) {
    let server = Router::new()
        .route("/address", get(|| async { address }))
        .nest_service("/", get(get_static_file))
        .layer(
            // allow cors
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET]),
        );

    axum::Server::bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), PORT))
        .serve(server.into_make_service())
        .await
        .unwrap();
}

async fn get_static_file(uri: Uri) -> Response<BoxBody> {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();

    log::info!("Serving static file: {}", req.uri());

    // `ServeDir` implements `tower::Service` so we can call it with `tower::ServiceExt::oneshot`
    match ServeDir::new("../client").oneshot(req).await {
        Ok(res) => res.map(boxed),
        Err(err) => match err {},
    }
}
