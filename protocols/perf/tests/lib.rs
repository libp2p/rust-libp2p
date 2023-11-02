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

use libp2p_perf::{
    client::{self},
    server, RunParams,
};
use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn perf() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut server = Swarm::new_ephemeral(|_| server::Behaviour::new());
    let server_peer_id = *server.local_peer_id();
    let mut client = Swarm::new_ephemeral(|_| client::Behaviour::new());

    server.listen().with_memory_addr_external().await;
    client.connect(&mut server).await;

    tokio::task::spawn(server.loop_on_next());

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

    client
        .wait(|e| match e {
            SwarmEvent::IncomingConnection { .. } => panic!(),
            SwarmEvent::ConnectionEstablished { .. } => None,
            SwarmEvent::Dialing { .. } => None,
            SwarmEvent::Behaviour(client::Event { result, .. }) => Some(result),
            e => panic!("{e:?}"),
        })
        .await
        .unwrap();
}
