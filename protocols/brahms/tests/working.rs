// Copyright 2019 Parity Technologies (UK) Ltd.
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

use futures::prelude::*;
use libp2p_brahms::{Brahms, BrahmsConfig, BrahmsViewSize};
use libp2p_core::{
    topology::MemoryTopology, upgrade, upgrade::OutboundUpgradeExt, Swarm, Transport,
};
use std::time::Duration;

#[test]
fn topology_filled() {
    /// Spawns a lot of nodes and test whether they discover each other.
    const NUM_SWARMS: usize = 15;

    let brahms_config = BrahmsConfig {
        view_size: BrahmsViewSize::from_network_size(NUM_SWARMS as u64),
        round_duration: Duration::from_secs(1),
        num_samplers: 32,
        difficulty: 10,
    };

    let mut swarms = Vec::with_capacity(NUM_SWARMS);
    for _ in 0..NUM_SWARMS {
        // TODO: make creating the transport more elegant ; literaly half of the code of the test
        //       is about creating the transport
        let local_key = libp2p_secio::SecioKeyPair::ed25519_generated().unwrap();
        let local_public_key = local_key.to_public_key();
        let transport = libp2p_tcp::TcpConfig::new()
            .with_upgrade(libp2p_secio::SecioConfig::new(local_key))
            .and_then(move |out, _| {
                let peer_id = out.remote_key.into_peer_id();
                let upgrade =
                    libp2p_mplex::MplexConfig::new().map_outbound(move |muxer| (peer_id, muxer));
                upgrade::apply_outbound(out.stream, upgrade)
            });

        // Each swarm contains the address of the previous swarm in its topology.
        let mut topology = MemoryTopology::empty(local_public_key.clone());
        if let Some(prev_swarm) = swarms.last_mut() {
            let addr =
                Swarm::listen_on(prev_swarm, "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
            topology.add_address(Swarm::local_peer_id(&prev_swarm).clone(), addr)
        }

        let brahms = Brahms::new(brahms_config, local_public_key.into_peer_id());
        swarms.push(Swarm::new(transport, brahms, topology));
    }

    Swarm::listen_on(
        swarms.last_mut().unwrap(),
        "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
    )
    .unwrap();

    let mut test_stop = tokio::timer::Interval::new_interval(Duration::from_secs(1));

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(futures::future::poll_fn(move || {
            for swarm in &mut swarms {
                while let Async::Ready(_) = swarm.poll().unwrap() {}
            }

            match test_stop.poll().unwrap() {
                Async::NotReady => Ok::<_, ()>(Async::NotReady),
                Async::Ready(_) => {
                    for swarm in &swarms {
                        if Swarm::topology(swarm).peers().count() != NUM_SWARMS - 1 {
                            return Ok(Async::NotReady)
                        }
                    }

                    Ok(Async::Ready(()))
                }
            }
        }))
        .unwrap();
}
