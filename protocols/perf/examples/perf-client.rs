// Copyright 2023 Protocol Labs.
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

use clap::Parser;
use futures::{executor::block_on, StreamExt};
use libp2p_core::{identity, upgrade, Multiaddr, PeerId, Transport};
use libp2p_dns::DnsConfig;
use libp2p_perf::RunParams;
use libp2p_swarm::{SwarmBuilder, SwarmEvent};
use log::info;

#[derive(Debug, Parser)]
#[clap(name = "libp2p perf client")]
struct Opts {
    #[arg(long)]
    server_address: Multiaddr,
}

fn main() {
    env_logger::init();

    let opts = Opts::parse();

    // Create a random PeerId
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {local_peer_id:?}");

    let transport = block_on(DnsConfig::system(libp2p_tcp::async_io::Transport::new(
        libp2p_tcp::Config::default().port_reuse(true),
    )))
    .unwrap()
    .upgrade(upgrade::Version::V1)
    .authenticate(
        libp2p_noise::NoiseAuthenticated::xx(&local_key)
            .expect("Signing libp2p-noise static DH keypair failed."),
    )
    .multiplex(libp2p_yamux::YamuxConfig::default())
    .boxed();

    let mut swarm = SwarmBuilder::with_async_std_executor(
        transport,
        libp2p_perf::client::behaviour::Behaviour::default(),
        local_peer_id,
    )
    .build();

    swarm.behaviour_mut().perf(
        opts.server_address.clone(),
        RunParams {
            to_send: 10 * 1024 * 1024,
            to_receive: 10 * 1024 * 1024,
        },
    );

    let stats = block_on(async {
        loop {
            match swarm.next().await.unwrap() {
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    info!("Established connection to {:?} via {:?}", peer_id, endpoint);
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                    info!("Outgoing connection error to {:?}: {:?}", peer_id, error);
                }
                SwarmEvent::Behaviour(libp2p_perf::client::behaviour::Event::Finished {
                    stats,
                }) => break stats,
                e => panic!("{e:?}"),
            }
        }
    });

    println!(
        "Finished run: Sent {} bytes and received {} bytes in {} seconds",
        stats.params.to_send,
        stats.params.to_receive,
        (stats.finished_at - stats.started_at).as_secs_f64()
    );
}
