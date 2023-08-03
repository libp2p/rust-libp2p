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
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_memory_connection_limits::*;
use util::*;

use libp2p_swarm::{dial_opts::DialOpts, DialError, Swarm};
use libp2p_swarm_test::SwarmExt;
use rand::{rngs::OsRng, Rng};

#[test]
fn max_bytes() {
    let connection_limit = OsRng.gen_range(1..30);
    let max_allowed_bytes = connection_limit * 1024 * 1024;

    let mut network = Swarm::new_ephemeral(|_| TestBehaviour {
        connection_limits: Behaviour::new_with_max_bytes(max_allowed_bytes)
            .with_memory_usage_refresh_interval(None),
        mem: Default::default(),
    });

    // Adds current mem usage to the limit and update
    let current_mem = memory_stats::memory_stats().unwrap().physical_mem;
    let max_allowed_bytes_plus_base_usage = max_allowed_bytes + current_mem;
    network
        .behaviour_mut()
        .connection_limits
        .update_max_bytes(max_allowed_bytes_plus_base_usage);

    let addr: Multiaddr = "/memory/1234".parse().unwrap();
    let target = PeerId::random();

    for _ in 0..connection_limit {
        network
            .dial(
                DialOpts::peer_id(target)
                    .addresses(vec![addr.clone()])
                    .build(),
            )
            .expect("Unexpected connection limit.");
    }

    match network
        .dial(DialOpts::peer_id(target).addresses(vec![addr]).build())
        .expect_err("Unexpected dialing success.")
    {
        DialError::Denied { cause } => {
            let exceeded = cause
                .downcast::<MemoryUsageLimitExceeded>()
                .expect("connection denied because of limit");

            assert_eq!(
                exceeded.max_allowed_bytes,
                max_allowed_bytes_plus_base_usage
            );
            assert!(exceeded.process_physical_memory_bytes >= exceeded.max_allowed_bytes);
        }
        e => panic!("Unexpected error: {e:?}"),
    }
}
