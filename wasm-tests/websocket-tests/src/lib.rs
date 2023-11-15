use std::env;
use libp2p_core::{StreamMuxer, Transport as _};
use libp2p_identity::Keypair;
use libp2p_noise as noise;
use std::error::Error;
use std::time::Duration;
use libp2p::{identify, SwarmBuilder, yamux};
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::SwarmEvent;
use anyhow::Context;
use futures::{select, StreamExt};
use futures_timer::Delay;
use futures::FutureExt;
use libp2p::swarm::NetworkBehaviour;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude")]
pub(crate) struct Behaviour {
    identify: identify::Behaviour,
}

fn build_behaviour(key: &Keypair) -> Behaviour {
    Behaviour {
        identify: identify::Behaviour::new(identify::Config::new(
            "ipfs/0.1.0".to_owned(),
            key.public(),
        )),
    }
}

async fn connect() -> Result<(), Box<dyn Error>> {

    let server_listen_multiaddr = env!("SERVER_LISTEN_MULTIADDRESS");

    #[cfg(not(target_arch = "wasm32"))]
    let swarm_builder = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_websocket(noise::Config::new, yamux::Config::default)
        .await?;

    #[cfg(target_arch = "wasm32")]
    let swarm_builder = SwarmBuilder::with_new_identity()
        .with_wasm_bindgen()
        .with_other_transport(|local_key| {
            Ok(libp2p_websocket_websys::Transport::default()
                .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                .authenticate(
                    noise::Config::new(&local_key)
                        .context("failed to initialise noise")?,
                )
                .multiplex(yamux::Config::default()))
            }
        )?;

    let mut swarm = swarm_builder
        .with_behaviour(build_behaviour)?
        .build();

    let opts = DialOpts::peer_id("12D3KooWNbGStz2dMnBZYdAvuchWW4xfeHrjHboP1hiq6XexRKEf".parse().context("failed to parse peer id")?)
        .condition(PeerCondition::Always)
        .addresses(vec![server_listen_multiaddr.parse().unwrap()])
        .extend_addresses_through_behaviour()
        .build();

    swarm.dial(opts).expect("failed to dial");

    let timeout = Duration::from_secs(30);
    let mut timeout_future = Delay::new(timeout).fuse();
    let mut is_successful = false;

    loop {
        select! {
            event = swarm.next() => {
                match event {
                    Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                        log::debug!("Connection established with {:?}", peer_id);
                    },
                    Some(SwarmEvent::Behaviour(BehaviourEvent::Identify(event))) => {
                        log::debug!("Identify event: {:?}", event);
                        is_successful = true;
                        break;
                    },
                    Some(other_event) => {
                        log::debug!("Other event received: {:?}", other_event);
                    },
                    None => break,  // In case the stream ends
                }
            }
            _ = timeout_future => {
                log::debug!("Timeout reached, exiting loop.");
                break;
            }
            complete => break, // Exit if all futures are complete
        }

        if is_successful {
            break;
        }
    };

    if is_successful {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Connection failed").into())
    }
}

#[tokio::test]
async fn test_with_tokio() -> Result<(), Box<dyn Error>> {
    let _ = env_logger::builder().is_test(true).try_init();
    connect().await
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
async fn test_with_wasm_bindgen() -> Result<(), Box<dyn Error>> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Debug));
    connect().await
}
