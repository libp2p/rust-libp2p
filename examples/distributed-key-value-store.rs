// Copyright 20l9 Parity Technologies (UK) Ltd.
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

//! A basic key value store demonstrating libp2p and the mDNS and Kademlia protocols.
//!
//! 1. Using two terminal windows, start two instances. If you local network
//!    allows mDNS, they will automatically connect.
//!
//! 2. Type `PUT my-key my-value` in terminal one and hit return.
//!
//! 3. Type `GET my-key` in terminal two and hit return.
//!
//! 4. Close with Ctrl-c.

use futures::prelude::*;
use libp2p::kad::record::store::MemoryStore;
use libp2p::kad::{record::Key, Kademlia, KademliaEvent, PutRecordOk, Quorum, Record};
use libp2p::{
    build_development_transport, identity,
    mdns::{Mdns, MdnsEvent},
    swarm::NetworkBehaviourEventProcess,
    tokio_codec::{FramedRead, LinesCodec},
    tokio_io::{AsyncRead, AsyncWrite},
    NetworkBehaviour, PeerId, Swarm,
};

fn main() {
    env_logger::init();

    // Create a random key for ourselves.
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    // Set up a an encrypted DNS-enabled TCP Transport over the Mplex protocol.
    let transport = build_development_transport(local_key);

    // We create a custom network behaviour that combines Kademlia and mDNS.
    #[derive(NetworkBehaviour)]
    struct MyBehaviour<TSubstream: AsyncRead + AsyncWrite> {
        kademlia: Kademlia<TSubstream, MemoryStore>,
        mdns: Mdns<TSubstream>,
    }

    impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<MdnsEvent>
        for MyBehaviour<TSubstream>
    {
        // Called when `mdns` produces an event.
        fn inject_event(&mut self, event: MdnsEvent) {
            if let MdnsEvent::Discovered(list) = event {
                for (peer_id, multiaddr) in list {
                    self.kademlia.add_address(&peer_id, multiaddr);
                }
            }
        }
    }

    impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<KademliaEvent>
        for MyBehaviour<TSubstream>
    {
        // Called when `kademlia` produces an event.
        fn inject_event(&mut self, message: KademliaEvent) {
            match message {
                KademliaEvent::GetRecordResult(Ok(result)) => {
                    for Record { key, value, .. } in result.records {
                        println!(
                            "Got record {:?} {:?}",
                            std::str::from_utf8(key.as_ref()).unwrap(),
                            std::str::from_utf8(&value).unwrap(),
                        );
                    }
                }
                KademliaEvent::GetRecordResult(Err(err)) => {
                    eprintln!("Failed to get record: {:?}", err);
                }
                KademliaEvent::PutRecordResult(Ok(PutRecordOk { key })) => {
                    println!(
                        "Successfully put record {:?}",
                        std::str::from_utf8(key.as_ref()).unwrap()
                    );
                }
                KademliaEvent::PutRecordResult(Err(err)) => {
                    eprintln!("Failed to put record: {:?}", err);
                }
                _ => {}
            }
        }
    }

    // Create a swarm to manage peers and events.
    let mut swarm = {
        // Create a Kademlia behaviour.
        let store = MemoryStore::new(local_peer_id.clone());
        let kademlia = Kademlia::new(local_peer_id.clone(), store);

        let behaviour = MyBehaviour {
            kademlia,
            mdns: Mdns::new().expect("Failed to create mDNS service"),
        };

        Swarm::new(transport, behaviour, local_peer_id)
    };

    // Read full lines from stdin.
    let stdin = tokio_stdin_stdout::stdin(0);
    let mut framed_stdin = FramedRead::new(stdin, LinesCodec::new());

    // Listen on all interfaces and whatever port the OS assigns.
    Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();

    // Kick it off.
    let mut listening = false;
    tokio::run(futures::future::poll_fn(move || {
        loop {
            match framed_stdin.poll().expect("Error while polling stdin") {
                Async::Ready(Some(line)) => {
                    handle_input_line(&mut swarm.kademlia, line);
                }
                Async::Ready(None) => panic!("Stdin closed"),
                Async::NotReady => break,
            };
        }

        loop {
            match swarm.poll().expect("Error while polling swarm") {
                Async::Ready(Some(_)) => {}
                Async::Ready(None) | Async::NotReady => {
                    if !listening {
                        if let Some(a) = Swarm::listeners(&swarm).next() {
                            println!("Listening on {:?}", a);
                            listening = true;
                        }
                    }
                    break;
                }
            }
        }

        Ok(Async::NotReady)
    }));
}

fn handle_input_line<TSubstream: AsyncRead + AsyncWrite>(
    kademlia: &mut Kademlia<TSubstream, MemoryStore>,
    line: String,
) {
    let mut args = line.split(" ");

    match args.next() {
        Some("GET") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            kademlia.get_record(&key, Quorum::One);
        }
        Some("PUT") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };
            let value = {
                match args.next() {
                    Some(value) => value.as_bytes().to_vec(),
                    None => {
                        eprintln!("Expected value");
                        return;
                    }
                }
            };
            let record = Record {
                key,
                value,
                publisher: None,
                expires: None,
            };
            kademlia.put_record(record, Quorum::One);
        }
        _ => {
            eprintln!("expected GET or PUT");
        }
    }
}
