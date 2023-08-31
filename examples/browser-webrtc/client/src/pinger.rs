use futures::{channel, SinkExt, StreamExt};
use libp2p::core::Multiaddr;
use libp2p::identity::{Keypair, PeerId};
use libp2p::ping;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent};
use std::convert::From;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

// The PORT that the server serves their Multiaddr
const PORT: u16 = 8080;
pub(crate) async fn start_pinger(
    mut sendr: channel::mpsc::Sender<Result<f32, Error>>,
) -> Result<(), Error> {
    let addr = fetch_server_addr().await.map_err(Error::FetchAddr)?;

    log::trace!("Got addr {} from server", addr);
    let local_key = Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let mut swarm = SwarmBuilder::with_wasm_executor(
        libp2p_webrtc_websys::Transport::new(libp2p_webrtc_websys::Config::new(&local_key)).boxed(),
        Behaviour {
            ping: ping::Behaviour::new(ping::Config::new()),
            keep_alive: keep_alive::Behaviour,
        },
        local_peer_id,
    )
    .build();

    log::info!("Running pinger with peer_id: {}", swarm.local_peer_id());

    log::info!("Dialing {}", addr);

    swarm.dial(addr.parse::<Multiaddr>()?)?;

    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event { result: Err(e), .. })) => {
                log::error!("Ping failed: {:?}", e);
                if sendr.send(Err(e.into())).await.is_err() {
                    break;
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            })) => {
                log::info!("Ping successful: RTT: {rtt:?}, from {peer}");
                if sendr.send(Ok(rtt.as_secs_f32())).await.is_err() {
                    break;
                }
            }
            evt => log::info!("Swarm event: {:?}", evt),
        }
    }

    Ok(())
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("failed to ping node")]
    Ping(#[from] ping::Failure),
    #[error("failed to parse address")]
    MultiaddrParse(#[from] libp2p::multiaddr::Error),
    #[error("failed to dial node")]
    Dial(#[from] libp2p::swarm::DialError),
    #[error("failed to fetch address: {0}")]
    FetchAddr(&'static str),
}

/// Helper that returns the multiaddress of echo-server
///
/// It fetches the multiaddress via HTTP request to
/// 127.0.0.1:4455.
async fn fetch_server_addr() -> Result<String, &'static str> {
    let url = format!("http://127.0.0.1:{}/address", PORT);
    let window = web_sys::window().expect("no global `window` exists");

    let response_promise = JsFuture::from(window.fetch_with_str(&url))
        .await
        .map_err(|_| "fetch failed")?
        .dyn_into::<web_sys::Response>()
        .map_err(|_| "cast to `Response` failed")?
        .text()
        .map_err(|_| "response is not text")?;

    let value = JsFuture::from(response_promise)
        .await
        .map_err(|_| "failed to retrieve body as text")?
        .as_string()
        .filter(|s| !s.is_empty())
        .ok_or("fetch text is empty")?;

    Ok(value)
}
