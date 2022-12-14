// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.use futures::StreamExt;

use futures::StreamExt;
use libp2p_core::{identity, upgrade::Version, PeerId, Transport};
use libp2p_mdns::Event;
use libp2p_mdns::{async_io::Behaviour, Config};
use libp2p_swarm::{Swarm, SwarmEvent};
use std::error::Error;
use std::time::Duration;

#[async_std::test]
async fn test_discovery_async_std_ipv4() -> Result<(), Box<dyn Error>> {
    run_discovery_test(Config::default()).await
}

#[async_std::test]
async fn test_discovery_async_std_ipv6() -> Result<(), Box<dyn Error>> {
    let config = Config {
        enable_ipv6: true,
        ..Default::default()
    };
    run_discovery_test(config).await
}

#[async_std::test]
async fn test_expired_async_std() -> Result<(), Box<dyn Error>> {
    env_logger::try_init().ok();
    let config = Config {
        ttl: Duration::from_secs(1),
        query_interval: Duration::from_secs(10),
        ..Default::default()
    };

    async_std::future::timeout(Duration::from_secs(6), run_peer_expiration_test(config))
        .await
        .map(|_| ())
        .map_err(|e| Box::new(e) as Box<dyn Error>)
}

async fn create_swarm(config: Config) -> Result<Swarm<Behaviour>, Box<dyn Error>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    let transport = libp2p_tcp::async_io::Transport::default()
        .upgrade(Version::V1)
        .authenticate(libp2p_noise::NoiseAuthenticated::xx(&id_keys).unwrap())
        .multiplex(libp2p_yamux::YamuxConfig::default())
        .boxed();
    let behaviour = Behaviour::new(config, peer_id)?;
    let mut swarm = Swarm::with_async_std_executor(transport, behaviour, peer_id);
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    Ok(swarm)
}

async fn run_discovery_test(config: Config) -> Result<(), Box<dyn Error>> {
    env_logger::try_init().ok();
    let mut a = create_swarm(config.clone()).await?;
    let mut b = create_swarm(config).await?;
    let mut discovered_a = false;
    let mut discovered_b = false;
    loop {
        futures::select! {
            ev = a.select_next_some() => if let SwarmEvent::Behaviour(Event::Discovered(peers)) = ev {
                for (peer, _addr) in peers {
                    if peer == *b.local_peer_id() {
                        if discovered_a {
                            return Ok(());
                        } else {
                            discovered_b = true;
                        }
                    }
                }
            },
            ev = b.select_next_some() => if let SwarmEvent::Behaviour(Event::Discovered(peers)) = ev {
                for (peer, _addr) in peers {
                    if peer == *a.local_peer_id() {
                        if discovered_b {
                            return Ok(());
                        } else {
                            discovered_a = true;
                        }
                    }
                }
            }
        }
    }
}

async fn run_peer_expiration_test(config: Config) -> Result<(), Box<dyn Error>> {
    let mut a = create_swarm(config.clone()).await?;
    let mut b = create_swarm(config).await?;

    loop {
        futures::select! {
            ev = a.select_next_some() => if let SwarmEvent::Behaviour(Event::Expired(peers)) = ev {
                for (peer, _addr) in peers {
                    if peer == *b.local_peer_id() {
                        return Ok(());
                    }
                }
            },
            ev = b.select_next_some() => if let SwarmEvent::Behaviour(Event::Expired(peers)) = ev {
                for (peer, _addr) in peers {
                    if peer == *a.local_peer_id() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
