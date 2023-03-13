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

use futures::{executor::LocalPool, task::Spawn, FutureExt, StreamExt};
use libp2p_perf::{
    client::{self, RunParams},
    server,
};
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;

#[test]
fn perf() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let mut server = Swarm::new_ephemeral(|_| server::Behaviour::new());
    let server_peer_id = *server.local_peer_id();
    let mut client = Swarm::new_ephemeral(|_| client::Behaviour::new());

    pool.run_until(server.listen());
    pool.run_until(client.connect(&mut server));

    pool.spawner()
        .spawn_obj(server.loop_on_next().boxed().into())
        .unwrap();

    client
        .behaviour_mut()
        .perf(
            server_peer_id,
            RunParams {
                to_send: 0,
                to_receive: 0,
            },
        )
        .unwrap();

    pool.run_until(async {
        loop {
            match client.select_next_some().await {
                SwarmEvent::IncomingConnection { .. } => panic!(),
                SwarmEvent::ConnectionEstablished { .. } => {}
                SwarmEvent::Dialing(_) => {}
                SwarmEvent::Behaviour(client::Event { result: Ok(_), .. }) => break,
                e => panic!("{e:?}"),
            }
        }
    });
}
