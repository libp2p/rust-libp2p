// Copyright 2020 Parity Technologies (UK) Ltd.
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

#![doc = include_str!("../README.md")]

use std::{env, error::Error, fs, path::Path, str::FromStr};

use either::Either;
use futures::prelude::*;
use libp2p::{
    core::transport::upgrade::Version,
    gossipsub, identify,
    multiaddr::Protocol,
    noise, ping,
    pnet::{PnetConfig, PreSharedKey},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, Transport,
};
use tokio::{io, io::AsyncBufReadExt, select};
use tracing_subscriber::EnvFilter;

/// Get the current ipfs repo path, either from the IPFS_PATH environment variable or
/// from the default $HOME/.ipfs
fn get_ipfs_path() -> Box<Path> {
    env::var("IPFS_PATH")
        .map(|ipfs_path| Path::new(&ipfs_path).into())
        .unwrap_or_else(|_| {
            env::var("HOME")
                .map(|home| Path::new(&home).join(".ipfs"))
                .expect("could not determine home directory")
                .into()
        })
}

/// Read the pre shared key file from the given ipfs directory
fn get_psk(path: &Path) -> std::io::Result<Option<String>> {
    let swarm_key_file = path.join("swarm.key");
    match fs::read_to_string(swarm_key_file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// for a multiaddr that ends with a peer id, this strips this suffix. Rust-libp2p
/// only supports dialing to an address without providing the peer id.
fn strip_peer_id(addr: &mut Multiaddr) {
    let last = addr.pop();
    match last {
        Some(Protocol::P2p(peer_id)) => {
            let mut addr = Multiaddr::empty();
            addr.push(Protocol::P2p(peer_id));
            println!("removing peer id {addr} so this address can be dialed by rust-libp2p");
        }
        Some(other) => addr.push(other),
        _ => {}
    }
}

/// parse a legacy multiaddr (replace ipfs with p2p), and strip the peer id
/// so it can be dialed by rust-libp2p
fn parse_legacy_multiaddr(text: &str) -> Result<Multiaddr, Box<dyn Error>> {
    let sanitized = text
        .split('/')
        .map(|part| if part == "ipfs" { "p2p" } else { part })
        .collect::<Vec<_>>()
        .join("/");
    let mut res = Multiaddr::from_str(&sanitized)?;
    strip_peer_id(&mut res);
    Ok(res)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let ipfs_path = get_ipfs_path();
    println!("using IPFS_PATH {ipfs_path:?}");
    let psk: Option<PreSharedKey> = get_psk(&ipfs_path)?
        .map(|text| PreSharedKey::from_str(&text))
        .transpose()?;

    if let Some(psk) = psk {
        println!("using swarm key with fingerprint: {}", psk.fingerprint());
    }

    // Create a Gosspipsub topic
    let gossipsub_topic = gossipsub::IdentTopic::new("chat");

    // We create a custom network behaviour that combines gossipsub, ping and identify.
    #[derive(NetworkBehaviour)]
    struct MyBehaviour {
        gossipsub: gossipsub::Behaviour,
        identify: identify::Behaviour,
        ping: ping::Behaviour,
    }

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_other_transport(|key| {
            let noise_config = noise::Config::new(key).unwrap();
            let yamux_config = yamux::Config::default();

            let base_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true));
            let maybe_encrypted = match psk {
                Some(psk) => Either::Left(
                    base_transport
                        .and_then(move |socket, _| PnetConfig::new(psk).handshake(socket)),
                ),
                None => Either::Right(base_transport),
            };
            maybe_encrypted
                .upgrade(Version::V1Lazy)
                .authenticate(noise_config)
                .multiplex(yamux_config)
        })?
        .with_dns()?
        .with_behaviour(|key| {
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .max_transmit_size(262144)
                .build()
                .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?; // Temporary hack because `build` does not return a proper `std::error::Error`.
            Ok(MyBehaviour {
                gossipsub: gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )
                .expect("Valid configuration"),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/ipfs/0.1.0".into(),
                    key.public(),
                )),
                ping: ping::Behaviour::new(ping::Config::new()),
            })
        })?
        .build();

    println!("Subscribing to {gossipsub_topic:?}");
    swarm
        .behaviour_mut()
        .gossipsub
        .subscribe(&gossipsub_topic)
        .unwrap();

    // Reach out to other nodes if specified
    for to_dial in std::env::args().skip(1) {
        let addr: Multiaddr = parse_legacy_multiaddr(&to_dial)?;
        swarm.dial(addr)?;
        println!("Dialed {to_dial:?}")
    }

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // Kick it off
    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                if let Err(e) = swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(gossipsub_topic.clone(), line.as_bytes())
                {
                    println!("Publish error: {e:?}");
                }
            },
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Listening on {address:?}");
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Identify(event)) => {
                        println!("identify: {event:?}");
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                        propagation_source: peer_id,
                        message_id: id,
                        message,
                    })) => {
                        println!(
                            "Got message: {} with id: {} from peer: {:?}",
                            String::from_utf8_lossy(&message.data),
                            id,
                            peer_id
                        )
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Ping(event)) => {
                        match event {
                            ping::Event {
                                peer,
                                result: Result::Ok(rtt),
                                ..
                            } => {
                                println!(
                                    "ping: rtt to {} is {} ms",
                                    peer.to_base58(),
                                    rtt.as_millis()
                                );
                            }
                            ping::Event {
                                peer,
                                result: Result::Err(ping::Failure::Timeout),
                                ..
                            } => {
                                println!("ping: timeout to {}", peer.to_base58());
                            }
                            ping::Event {
                                peer,
                                result: Result::Err(ping::Failure::Unsupported),
                                ..
                            } => {
                                println!("ping: {} does not support ping protocol", peer.to_base58());
                            }
                            ping::Event {
                                peer,
                                result: Result::Err(ping::Failure::Other { error }),
                                ..
                            } => {
                                println!("ping: ping::Failure with {}: {error}", peer.to_base58());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
