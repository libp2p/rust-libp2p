// Copyright 2017 Parity Technologies (UK) Ltd.
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

extern crate bigint;
extern crate bytes;
extern crate env_logger;
extern crate example;
extern crate futures;
extern crate libp2p_identify as identify;
extern crate libp2p_kad as kad;
extern crate libp2p_peerstore as peerstore;
extern crate libp2p_secio as secio;
extern crate libp2p_swarm as swarm;
extern crate libp2p_tcp_transport as tcp;
extern crate multiplex;
extern crate tokio_core;
extern crate tokio_io;

use bigint::U512;
use futures::future::Future;
use peerstore::PeerId;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use swarm::{Transport, UpgradeExt};
use tcp::TcpConfig;
use tokio_core::reactor::Core;

fn main() {
    env_logger::init();

    // Determine which addresses to listen to.
    let listen_addrs = {
        let mut args = env::args().skip(1).collect::<Vec<_>>();
        if args.is_empty() {
            args.push("/ip4/0.0.0.0/tcp/0".to_owned());
        }
        args
    };

    // We start by building the tokio engine that will run all the sockets.
    let mut core = Core::new().unwrap();

    let peer_store = Arc::new(peerstore::memory_peerstore::MemoryPeerstore::empty());
    example::ipfs_bootstrap(&*peer_store);

    // Now let's build the transport stack.
    // We create a `TcpConfig` that indicates that we want TCP/IP.
    let transport = identify::IdentifyTransport::new(
        TcpConfig::new(core.handle())

        // On top of TCP/IP, we will use either the plaintext protocol or the secio protocol,
        // depending on which one the remote supports.
        .with_upgrade({
            let plain_text = swarm::PlainTextConfig;

            let secio = {
                let private_key = include_bytes!("test-private-key.pk8");
                let public_key = include_bytes!("test-public-key.der").to_vec();
                secio::SecioConfig {
                    key: secio::SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
                }
            };

            plain_text.or_upgrade(secio)
        })

        // On top of plaintext or secio, we will use the multiplex protocol.
        .with_upgrade(multiplex::MultiplexConfig)
        // The object returned by the call to `with_upgrade(MultiplexConfig)` can't be used as a
        // `Transport` because the output of the upgrade is not a stream but a controller for
        // muxing. We have to explicitly call `into_connection_reuse()` in order to turn this into
        // a `Transport`.
        .into_connection_reuse(),
        peer_store.clone(),
    );

    // We now have a `transport` variable that can be used either to dial nodes or listen to
    // incoming connections, and that will automatically apply secio and multiplex on top
    // of any opened stream.

    let my_peer_id = PeerId::from_public_key(include_bytes!("test-public-key.der"));
    println!("Local peer id is: {:?}", my_peer_id);

    // Let's put this `transport` into a Kademlia *swarm*. The swarm will handle all the incoming
    // and outgoing connections for us.
    let kad_config = kad::KademliaConfig {
        parallelism: 3,
        record_store: (),
        peer_store: peer_store,
        local_peer_id: my_peer_id.clone(),
        timeout: Duration::from_secs(2),
    };

    let kad_ctl_proto = kad::KademliaControllerPrototype::new(kad_config);

    let proto = kad::KademliaUpgrade::from_prototype(&kad_ctl_proto);

    // Let's put this `transport` into a *swarm*. The swarm will handle all the incoming and
    // outgoing connections for us.
    let (swarm_controller, swarm_future) = swarm::swarm(transport, proto, |upgrade, _| upgrade);

    let (kad_controller, _kad_init) = kad_ctl_proto.start(swarm_controller.clone());

    for listen_addr in listen_addrs {
        let addr = swarm_controller
            .listen_on(listen_addr.parse().expect("invalid multiaddr"))
            .expect("unsupported multiaddr");
        println!("Now listening on {:?}", addr);
    }

    let finish_enum = kad_controller
        .find_node(my_peer_id.clone())
        .and_then(|out| {
            let local_hash = U512::from(my_peer_id.hash());
            println!("Results of peer discovery for {:?}:", my_peer_id);
            for n in out {
                let other_hash = U512::from(n.hash());
                let dist = 512 - (local_hash ^ other_hash).leading_zeros();
                println!("* {:?} (distance bits = {:?} (lower is better))", n, dist);
            }
            Ok(())
        });

    // `swarm_future` is a future that contains all the behaviour that we want, but nothing has
    // actually started yet. Because we created the `TcpConfig` with tokio, we need to run the
    // future through the tokio core.
    core.run(
        finish_enum
            .select(swarm_future)
            .map(|(n, _)| n)
            .map_err(|(err, _)| err),
    ).unwrap();
}
