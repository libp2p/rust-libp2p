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

mod util;

use std::time::Duration;

use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_memory_connection_limits::*;
use libp2p_swarm::{
    dial_opts::{DialOpts, PeerCondition},
    DialError, Swarm,
};
use libp2p_swarm_test::SwarmExt;
use sysinfo::{MemoryRefreshKind, RefreshKind};
use util::*;

#[test]
fn max_percentage() {
    const CONNECTION_LIMIT: usize = 20;
    let system_info = sysinfo::System::new_with_specifics(
        RefreshKind::default().with_memory(MemoryRefreshKind::default().with_ram()),
    );

    let mut network = Swarm::new_ephemeral(|_| TestBehaviour {
        connection_limits: Behaviour::with_max_percentage(0.1),
        mem_consumer: ConsumeMemoryBehaviour1MBPending0Established::default(),
    });

    let addr: Multiaddr = "/memory/1234".parse().unwrap();
    let target = PeerId::random();

    // Exercise `dial` function to get more stable memory stats later
    network
        .dial(
            DialOpts::peer_id(target)
                .addresses(vec![addr.clone()])
                .build(),
        )
        .expect("Unexpected connection limit.");

    // Adds current mem usage to the limit and update
    let current_mem = memory_stats::memory_stats().unwrap().physical_mem;
    let max_allowed_bytes = current_mem + CONNECTION_LIMIT * 1024 * 1024;
    network.behaviour_mut().connection_limits = Behaviour::with_max_percentage(
        max_allowed_bytes as f64 / system_info.total_memory() as f64,
    );

    for _ in 0..CONNECTION_LIMIT {
        network
            .dial(
                DialOpts::peer_id(target)
                    // Always dial, even if already dialing or connected.
                    .condition(PeerCondition::Always)
                    .addresses(vec![addr.clone()])
                    .build(),
            )
            .expect("Unexpected connection limit.");
    }

    // Memory stats are only updated every 100ms internally,
    // ensure they are up-to-date when we try to exceed it.
    std::thread::sleep(Duration::from_millis(100));

    match network
        .dial(
            DialOpts::peer_id(target)
                .condition(PeerCondition::Always)
                .addresses(vec![addr])
                .build(),
        )
        .expect_err("Unexpected dialing success.")
    {
        DialError::Denied { cause } => {
            let exceeded = cause
                .downcast::<MemoryUsageLimitExceeded>()
                .expect("connection denied because of limit");

            assert_eq!(exceeded.max_allowed_bytes(), max_allowed_bytes);
            assert!(exceeded.process_physical_memory_bytes() >= exceeded.max_allowed_bytes());
        }
        e => panic!("Unexpected error: {e:?}"),
    }
}
