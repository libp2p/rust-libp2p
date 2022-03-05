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

//! Example demonstrating `libp2p-metrics`.
//!
//! In one terminal run:
//!
//! ```
//! cargo run --example metrics
//! ```
//!
//! In a second terminal run:
//!
//! ```
//! cargo run --example metrics -- <listen-addr-of-first-node>
//! ```
//!
//! Where `<listen-addr-of-first-node>` is replaced by the listen address of the
//! first node reported in the first terminal. Look for `NewListenAddr`.
//!
//! In a third terminal run:
//!
//! ```
//! curl localhost:<metrics-port-of-first-or-second-node>/metrics
//! ```
//!
//! Where `<metrics-port-of-first-or-second-node>` is replaced by the listen
//! port of the metrics server of the first or the second node. Look for
//! `tide::server Server listening on`.
//!
//! You should see a long list of metrics printed to the terminal. Check the
//! `libp2p_ping` metrics, they should be `>0`.

use futures::executor::block_on;
use futures::stream::StreamExt;
use libp2p::core::Multiaddr;
use libp2p::metrics::{Metrics, Recorder};
use libp2p::ping::{Ping, PingConfig};
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, PeerId, Swarm};
use prometheus_client::registry::Registry;
use std::error::Error;
use std::thread;

use env_logger::Env;
use log::info;
mod http_service;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {:?}", local_peer_id);

    let mut swarm = Swarm::new(
        block_on(libp2p::development_transport(local_key))?,
        Ping::new(PingConfig::new().with_keep_alive(true)),
        local_peer_id,
    );

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
                SwarmEvent::Behaviour(ping_event) => {
                    info!("{:?}", ping_event);
                    metrics.record(&ping_event);
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
