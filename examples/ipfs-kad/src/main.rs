// Copyright 2018 Parity Technologies (UK) Ltd.
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

use std::error::Error;
use std::num::NonZeroUsize;
use std::ops::Add;
use std::time::{Duration, Instant};

use clap::Parser;
use futures::StreamExt;

use libp2p::bytes::BufMut;
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{development_transport, identity, kad, PeerId};

const BOOTNODES: [&str; 4] = [
    "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
    "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
    "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
    "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
];

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    // Create a random key for ourselves.
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    // Set up a an encrypted DNS-enabled TCP Transport over the yamux protocol
    let transport = development_transport(local_key.clone()).await?;

    // Create a swarm to manage peers and events.
    let mut swarm = {
        // Create a Kademlia behaviour.
        let mut cfg = kad::Config::default();
        cfg.set_query_timeout(Duration::from_secs(5 * 60));
        let store = kad::store::MemoryStore::new(local_peer_id);
        let mut behaviour = kad::Behaviour::with_config(local_peer_id, store, cfg);

        // Add the bootnodes to the local routing table. `libp2p-dns` built
        // into the `transport` resolves the `dnsaddr` when Kademlia tries
        // to dial these nodes.
        for peer in &BOOTNODES {
            behaviour.add_address(&peer.parse()?, "/dnsaddr/bootstrap.libp2p.io".parse()?);
        }

        SwarmBuilder::with_async_std_executor(transport, behaviour, local_peer_id).build()
    };

    let cli_opt = Opt::parse();

    match cli_opt.argument {
        CliArgument::GetPeers { peer_id } => {
            let peer_id = peer_id.unwrap_or(PeerId::random());
            println!("Searching for the closest peers to {peer_id}");
            swarm.behaviour_mut().get_closest_peers(peer_id);
        }
        CliArgument::PutPkRecord {} => {
            println!("Putting PK record into the DHT");

            let mut pk_record_key = vec![];
            pk_record_key.put_slice("/pk/".as_bytes());
            pk_record_key.put_slice(local_peer_id.to_bytes().as_slice());

            let mut pk_record =
                kad::Record::new(pk_record_key, local_key.public().encode_protobuf());
            pk_record.publisher = Some(local_peer_id);
            pk_record.expires = Some(Instant::now().add(Duration::from_secs(60)));

            swarm
                .behaviour_mut()
                .put_record(pk_record, kad::Quorum::N(NonZeroUsize::new(3).unwrap()))?;
        }
    }

    loop {
        let event = swarm.select_next_some().await;

        match event {
            SwarmEvent::Behaviour(kad::Event::OutboundQueryProgressed {
                result: kad::QueryResult::GetClosestPeers(Ok(ok)),
                ..
            }) => {
                if !ok.peers.is_empty() {
                    println!("Query finished with closest peers: {:#?}", ok.peers)
                } else {
                    // The example is considered failed as there
                    // should always be at least 1 reachable peer.
                    println!("Query finished with no closest peers.")
                }

                break;
            }
            SwarmEvent::Behaviour(kad::Event::OutboundQueryProgressed {
                result:
                    kad::QueryResult::GetClosestPeers(Err(kad::GetClosestPeersError::Timeout {
                        peers,
                        ..
                    })),
                ..
            }) => {
                if !peers.is_empty() {
                    println!("Query timed out with closest peers: {peers:#?}")
                } else {
                    // The example is considered failed as there
                    // should always be at least 1 reachable peer.
                    println!("Query timed out with no closest peers.");
                }
                break;
            }
            SwarmEvent::Behaviour(kad::Event::OutboundQueryProgressed {
                result: kad::QueryResult::PutRecord(Ok(_)),
                ..
            }) => {
                println!("Successfully inserted the PK record");

                break;
            }
            SwarmEvent::Behaviour(kad::Event::OutboundQueryProgressed {
                result: kad::QueryResult::PutRecord(Err(err)),
                ..
            }) => {
                println!("Failed to insert the PK record {err}");

                break;
            }

            _ => continue,
        }
    }

    Ok(())
}

#[derive(Parser, Debug)]
#[clap(name = "libp2p Kademlia DHT example")]
struct Opt {
    #[clap(subcommand)]
    argument: CliArgument,
}

#[derive(Debug, Parser)]
enum CliArgument {
    GetPeers {
        #[clap(long)]
        peer_id: Option<PeerId>,
    },
    PutPkRecord {},
}
