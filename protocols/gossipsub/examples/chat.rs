extern crate env_logger;
extern crate futures;
extern crate libp2p;
extern crate log;
extern crate tokio;

use env_logger::{Builder, Env};
use futures::prelude::*;
use libp2p::gossipsub::GossipsubEvent;
use libp2p::{
    gossipsub, secio,
    tokio_codec::{FramedRead, LinesCodec},
};
use std::time::Duration;

fn main() {
    Builder::from_env(Env::default().default_filter_or("debug")).init();

    // Create a random PeerId
    let local_key = secio::SecioKeyPair::ed25519_generated().unwrap();
    let local_peer_id = local_key.to_peer_id();
    println!("Local peer id: {:?}", local_peer_id);

    // Set up an encrypted TCP Transport over the Mplex and Yamux protocols
    let transport = libp2p::build_development_transport(local_key);

    // Create a Floodsub/Gossipsub topic
    let topic = libp2p::floodsub::TopicBuilder::new("test").build();

    // Create a Swarm to manage peers and events
    let mut swarm = {
        // set default parameters for gossipsub
        //let gossipsub_config = gossipsub::GossipsubConfig::default();
        // set custom gossipsub
        let gossipsub_config = gossipsub::GossipsubConfig::new(
            5,
            3,
            6,
            4,
            12,
            6,
            Duration::from_secs(10),
            Duration::from_secs(10),
            Duration::from_secs(60),
        );
        // build a gossipsub network behaviour
        let mut gossipsub = gossipsub::Gossipsub::new(local_peer_id.clone(), gossipsub_config);
        gossipsub.subscribe(topic.clone());
        libp2p::Swarm::new(transport, gossipsub, local_peer_id)
    };

    // Listen on all interfaces and whatever port the OS assigns
    let addr = libp2p::Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();
    println!("Listening on {:?}", addr);

    // Reach out to another node if specified
    if let Some(to_dial) = std::env::args().nth(1) {
        let dialing = to_dial.clone();
        match to_dial.parse() {
            Ok(to_dial) => match libp2p::Swarm::dial_addr(&mut swarm, to_dial) {
                Ok(_) => println!("Dialed {:?}", dialing),
                Err(e) => println!("Dial {:?} failed: {:?}", dialing, e),
            },
            Err(err) => println!("Failed to parse address to dial: {:?}", err),
        }
    }

    // Read full lines from stdin
    let stdin = tokio_stdin_stdout::stdin(0);
    let mut framed_stdin = FramedRead::new(stdin, LinesCodec::new());

    // Kick it off
    tokio::run(futures::future::poll_fn(move || -> Result<_, ()> {
        loop {
            match framed_stdin.poll().expect("Error while polling stdin") {
                Async::Ready(Some(line)) => swarm.publish(&topic, line.as_bytes()),
                Async::Ready(None) => panic!("Stdin closed"),
                Async::NotReady => break,
            };
        }

        loop {
            match swarm.poll().expect("Error while polling swarm") {
                Async::Ready(Some(gossip_event)) => match gossip_event {
                    GossipsubEvent::Message(message) => {
                        println!("Got message: {:?}", String::from_utf8_lossy(&message.data))
                    }
                    _ => {}
                },
                Async::Ready(None) | Async::NotReady => break,
            }
        }

        Ok(Async::NotReady)
    }));
}
