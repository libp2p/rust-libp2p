// Copyright 2021 Protocol Labs.
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
// DEALINGS IN THE SOFTWARE.

#![doc = include_str!("../README.md")]

use env_logger::Env;
use futures::{executor::block_on, StreamExt};
use libp2p::core::Multiaddr;
use libp2p::metrics::{Metrics, Recorder};
use libp2p::swarm::{self as swarm, NetworkBehaviour, SwarmEvent};
use libp2p::{identify, identity, noise, ping, tcp, yamux};
use log::info;
use prometheus_client::registry::Registry;
use std::error::Error;
use std::thread;
use std::time::Duration;

mod http_service;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_async_std()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| Behaviour::new(key.public()))?
        .with_swarm_config(
            swarm::Config::with_async_std_executor()
                .with_idle_connection_timeout(Duration::from_secs(u64::MAX)),
        )
        .build();

    info!("Local peer id: {:?}", swarm.local_peer_id());

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        info!("Dialed {}", addr)
    }

    let mut metric_registry = Registry::default();
    let metrics = Metrics::new(&mut metric_registry);
    thread::spawn(move || block_on(http_service::metrics_server(metric_registry)));

    block_on(async {
        loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(BehaviourEvent::Ping(ping_event)) => {
                    info!("{:?}", ping_event);
                    metrics.record(&ping_event);
                }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify_event)) => {
                    info!("{:?}", identify_event);
                    metrics.record(&identify_event);
                }
                swarm_event => {
                    info!("{:?}", swarm_event);
                    metrics.record(&swarm_event);
                }
            }
        }
    });
    Ok(())
}

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour {
    identify: identify::Behaviour,
    ping: ping::Behaviour,
}

impl Behaviour {
    fn new(local_pub_key: identity::PublicKey) -> Self {
        Self {
            ping: ping::Behaviour::default(),
            identify: identify::Behaviour::new(identify::Config::new(
                "/ipfs/0.1.0".into(),
                local_pub_key,
            )),
        }
    }
}
