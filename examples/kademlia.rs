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

// Libp2p's code unfortunately produces very large types. Rust's default length limit for type
// names is not large enough, therefore we need this attribute.
#![type_length_limit = "4194304"]

extern crate bigint;
extern crate bytes;
extern crate env_logger;
extern crate futures;
extern crate libp2p;
extern crate tokio_current_thread;
extern crate tokio_io;
extern crate multiaddr;

use bigint::U512;
use futures::{Future, Stream};
use libp2p::peerstore::{PeerAccess, PeerId, Peerstore};
use multiaddr::Multiaddr;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use libp2p::core::{Transport, PublicKey, UniqueConnec};
use libp2p::core::{upgrade, either::EitherOutput};
use libp2p::kad::{KadConnecConfig, KadConnectionType, KadPeer, KadQueryEvent, KadSystem};
use libp2p::kad::{KadSystemConfig, KadIncomingRequest};
use libp2p::tcp::TcpConfig;

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

    let peer_store = Arc::new(libp2p::peerstore::memory_peerstore::MemoryPeerstore::empty());
    ipfs_bootstrap(&*peer_store);

    // We create a `TcpConfig` that indicates that we want TCP/IP.
    let transport = TcpConfig::new()

        // On top of TCP/IP, we will use either the plaintext protocol or the secio protocol,
        // depending on which one the remote supports.
        .with_upgrade({
            let plain_text = upgrade::PlainTextConfig;

            let secio = {
                let private_key = include_bytes!("test-rsa-private-key.pk8");
                let public_key = include_bytes!("test-rsa-public-key.der").to_vec();
                libp2p::secio::SecioConfig {
                    key: libp2p::secio::SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
                }
            };

            upgrade::or(
                upgrade::map(plain_text, |pt| EitherOutput::First(pt)),
                upgrade::map(secio, |out: libp2p::secio::SecioOutput<_>| EitherOutput::Second(out.stream))
            )
        })

        // On top of plaintext or secio, we will use the multiplex protocol.
        .with_upgrade(libp2p::mplex::MplexConfig::new())
        // The object returned by the call to `with_upgrade(MplexConfig::new())` can't be used as a
        // `Transport` because the output of the upgrade is not a stream but a controller for
        // muxing. We have to explicitly call `into_connection_reuse()` in order to turn this into
        // a `Transport`.
        .map(|val, _| ((), val))
        .into_connection_reuse()
        .map(|((), val), _| val);

    let addr_resolver = {
        let peer_store = peer_store.clone();
        move |peer_id| {
            peer_store
                .peer(&peer_id)
                .into_iter()
                .flat_map(|peer| peer.addrs())
                .collect::<Vec<_>>()
                .into_iter()
        }
    };

    let transport = libp2p::identify::PeerIdTransport::new(transport, addr_resolver)
        .and_then({
            let peer_store = peer_store.clone();
            move |id_out, _, remote_addr| {
                let socket = id_out.socket;
                let original_addr = id_out.original_addr;
                id_out.info.map(move |info| {
                    let peer_id = info.info.public_key.into_peer_id();
                    peer_store.peer_or_create(&peer_id).add_addr(original_addr, Duration::from_secs(3600));
                    (socket, remote_addr)
                })
            }
        });

    // We now have a `transport` variable that can be used either to dial nodes or listen to
    // incoming connections, and that will automatically apply secio and multiplex on top
    // of any opened stream.

    let my_peer_id = PeerId::from_public_key(PublicKey::Rsa(include_bytes!("test-rsa-public-key.der").to_vec()));
    println!("Local peer id is: {:?}", my_peer_id);

    let kad_system = Arc::new(KadSystem::without_init(KadSystemConfig {
        parallelism: 3,
        local_peer_id: my_peer_id.clone(),
        kbuckets_timeout: Duration::from_secs(10),
        request_timeout: Duration::from_secs(10),
        known_initial_peers: peer_store.peers(),
    }));

    let active_kad_connections = Arc::new(Mutex::new(HashMap::<_, UniqueConnec<_>>::new()));

    // Let's put this `transport` into a *swarm*. The swarm will handle all the incoming and
    // outgoing connections for us.
    let (swarm_controller, swarm_future) = libp2p::core::swarm(
        transport.clone().with_upgrade(KadConnecConfig::new()),
        {
            let peer_store = peer_store.clone();
            let kad_system = kad_system.clone();
            let active_kad_connections = active_kad_connections.clone();
            move |(kad_ctrl, kad_stream), node_addr| {
                let peer_store = peer_store.clone();
                let kad_system = kad_system.clone();
                let active_kad_connections = active_kad_connections.clone();
                node_addr.and_then(move |node_addr| {
                    let node_id = p2p_multiaddr_to_node_id(node_addr);
                    let node_id2 = node_id.clone();
                    let fut = kad_stream.for_each(move |req| {
                        let peer_store = peer_store.clone();
                        kad_system.update_kbuckets(node_id2.clone());
                        match req {
                            KadIncomingRequest::FindNode { searched, responder } => {
                                let result = kad_system
                                    .known_closest_peers(&searched)
                                    .map(move |peer_id| {
                                        let addrs = peer_store
                                            .peer(&peer_id)
                                            .into_iter()
                                            .flat_map(|p| p.addrs())
                                            .collect::<Vec<_>>();
                                        KadPeer {
                                            node_id: peer_id.clone(),
                                            multiaddrs: addrs,
                                            connection_ty: KadConnectionType::Connected, // meh :-/
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                responder.respond(result);
                            },
                            KadIncomingRequest::PingPong => {
                            }
                        };
                        Ok(())
                    });

                    let mut active_kad_connections = active_kad_connections.lock().unwrap();
                    active_kad_connections
                        .entry(node_id)
                        .or_insert_with(Default::default)
                        .tie_or_passthrough(kad_ctrl, fut)
                })
            }
        }
    );

    for listen_addr in listen_addrs {
        let addr = swarm_controller
            .listen_on(listen_addr.parse().expect("invalid multiaddr"))
            .expect("unsupported multiaddr");
        println!("Now listening on {:?}", addr);
    }

    let finish_enum = kad_system
        .find_node(my_peer_id.clone(), |peer| {
            let addr = Multiaddr::from(libp2p::multiaddr::AddrComponent::P2P(peer.clone().into()));
            active_kad_connections.lock().unwrap().entry(peer.clone())
                .or_insert_with(Default::default)
                .dial(&swarm_controller, &addr, transport.clone().with_upgrade(KadConnecConfig::new()))
        })
        .filter_map(move |event| {
            match event {
                KadQueryEvent::NewKnownMultiaddrs(peers) => {
                    for (peer, addrs) in peers {
                        peer_store.peer_or_create(&peer)
                            .add_addrs(addrs, Duration::from_secs(3600));
                    }
                    None
                },
                KadQueryEvent::Finished(out) => Some(out),
            }
        })
        .into_future()
        .map_err(|(err, _)| err)
        .map(|(out, _)| out.unwrap())
        .and_then(|out| {
            let local_hash = U512::from(my_peer_id.digest());
            println!("Results of peer discovery for {:?}:", my_peer_id);
            for n in out {
                let other_hash = U512::from(n.digest());
                let dist = 512 - (local_hash ^ other_hash).leading_zeros();
                println!("* {:?} (distance bits = {:?} (lower is better))", n, dist);
            }
            Ok(())
        });

    // `swarm_future` is a future that contains all the behaviour that we want, but nothing has
    // actually started yet. Because we created the `TcpConfig` with tokio, we need to run the
    // future through the tokio core.
    tokio_current_thread::block_on_all(
        finish_enum
            .select(swarm_future.for_each(|_| Ok(())))
            .map(|(n, _)| n)
            .map_err(|(err, _)| err),
    ).unwrap();
}

/// Expects a multiaddr of the format `/p2p/<node_id>` and returns the node ID.
/// Panics if the format is not correct.
fn p2p_multiaddr_to_node_id(client_addr: Multiaddr) -> PeerId {
	let (first, second);
	{
		let mut iter = client_addr.iter();
		first = iter.next();
		second = iter.next();
	}
	match (first, second) {
		(Some(libp2p::multiaddr::AddrComponent::P2P(node_id)), None) =>
			PeerId::from_multihash(node_id).expect("libp2p always reports a valid node id"),
		_ => panic!("Reported multiaddress is in the wrong format ; programmer error")
	}
}

/// Stores initial addresses on the given peer store. Uses a very large timeout.
pub fn ipfs_bootstrap<P>(peer_store: P)
where
    P: Peerstore + Clone,
{
    const ADDRESSES: &[&str] = &[
        "/ip4/127.0.0.1/tcp/4001/p2p/QmQRx32wQkw3hB45j4UDw8V9Ju4mGbxMyhs2m8mpFrFkur",
        // TODO: add some bootstrap nodes here
    ];

    let ttl = Duration::from_secs(100 * 365 * 24 * 3600);

    for address in ADDRESSES.iter() {
        let mut multiaddr = address
            .parse::<Multiaddr>()
            .expect("failed to parse hard-coded multiaddr");

        let p2p_component = multiaddr.pop().expect("hard-coded multiaddr is empty");
        let peer = match p2p_component {
            libp2p::multiaddr::AddrComponent::P2P(key) => {
                PeerId::from_multihash(key).expect("invalid peer id")
            }
            _ => panic!("hard-coded multiaddr didn't end with /p2p/"),
        };

        peer_store
            .clone()
            .peer_or_create(&peer)
            .add_addr(multiaddr, ttl.clone());
    }
}
