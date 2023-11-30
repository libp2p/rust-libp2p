#![allow(non_upper_case_globals)]

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::{http::Method, routing::get, Router};
use futures::StreamExt;
use libp2p::{
    core::muxing::StreamMuxerBox,
    core::Transport,
    multiaddr::{Multiaddr, Protocol},
    ping,
    swarm::SwarmEvent,
};
use libp2p_webrtc as webrtc;
use rand::thread_rng;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("browser_webrtc_example=debug,libp2p_webrtc=info,libp2p_ping=debug")
        .try_init();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_other_transport(|id_keys| {
            Ok(webrtc::tokio::Transport::new(
                id_keys.clone(),
                webrtc::tokio::Certificate::generate(&mut thread_rng())?,
            )
            .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn))))
        })?
        .with_behaviour(|_| ping::Behaviour::default())?
        .with_swarm_config(|cfg| {
            cfg.with_idle_connection_timeout(
                Duration::from_secs(u64::MAX), // Allows us to observe the pings.
            )
        })
        .build();

    let address_webrtc = Multiaddr::from(Ipv4Addr::UNSPECIFIED)
        .with(Protocol::Udp(0))
        .with(Protocol::WebRTCDirect);

    swarm.listen_on(address_webrtc.clone())?;

    let address = loop {
        if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
            if address
                .iter()
                .any(|e| e == Protocol::Ip4(Ipv4Addr::LOCALHOST))
            {
                tracing::debug!(
                    "Ignoring localhost address to make sure the example works in Firefox"
                );
                continue;
            }

            tracing::info!(%address, "Listening");

            break address;
        }
    };

    let addr = address.with(Protocol::P2p(*swarm.local_peer_id()));

    // Serve .wasm, .js and server multiaddress over HTTP on this address.
    tokio::spawn(serve(addr));

    loop {
        tokio::select! {
            swarm_event = swarm.next() => {
                tracing::trace!(?swarm_event)
            },
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    Ok(())
}

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/static"]
struct StaticFiles;

/// Serve the Multiaddr we are listening on and the host files.
pub(crate) async fn serve(libp2p_transport: Multiaddr) {
    let Some(Protocol::Ip4(listen_addr)) = libp2p_transport.iter().next() else {
        panic!("Expected 1st protocol to be IP4")
    };

    let server = Router::new()
        .route("/", get(get_index))
        .route("/index.html", get(get_index))
        .route("/:path", get(get_static_file))
        .with_state(Libp2pEndpoint(libp2p_transport))
        .layer(
            // allow cors
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET]),
        );

    let addr = SocketAddr::new(listen_addr.into(), 8080);

    tracing::info!(url=%format!("http://{addr}"), "Serving client files at url");

    axum::Server::bind(&addr)
        .serve(server.into_make_service())
        .await
        .unwrap();
}

#[derive(Clone)]
struct Libp2pEndpoint(Multiaddr);

/// Serves the index.html file for our client.
///
/// Our server listens on a random UDP port for the WebRTC transport.
/// To allow the client to connect, we replace the `__LIBP2P_ENDPOINT__` placeholder with the actual address.
async fn get_index(
    State(Libp2pEndpoint(libp2p_endpoint)): State<Libp2pEndpoint>,
) -> Result<Html<String>, StatusCode> {
    let content = StaticFiles::get("index.html")
        .ok_or(StatusCode::NOT_FOUND)?
        .data;

    let html = std::str::from_utf8(&content)
        .expect("index.html to be valid utf8")
        .replace("__LIBP2P_ENDPOINT__", &libp2p_endpoint.to_string());

    Ok(Html(html))
}

/// Serves the static files generated by `wasm-pack`.
async fn get_static_file(Path(path): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    tracing::debug!(file_path=%path, "Serving static file");

    let content = StaticFiles::get(&path).ok_or(StatusCode::NOT_FOUND)?.data;
    let content_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    Ok(([(CONTENT_TYPE, content_type)], content))
}
