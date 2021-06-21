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

//! A basic relay server and relay client implementation.
//!
//! The example below involves three nodes: (1) a relay server, (2) a listening
//! relay client listening via the relay server and (3) a dialing relay client
//! dialing the listening relay client via the relay server.
//!
//! 1. To start the relay server, run `cargo run --example relay -- relay` which will print
//!    something along the lines of:
//!
//!    ```
//!    Local peer id: PeerId("12D3KooWAP5X5k9DS94n7AsiUAsaiso59Kioh14j2c13fCiudjdZ")
//!    #                      ^-- <peer-id-relay-server>
//!    Listening on "/ip6/::1/tcp/36537"
//!    #             ^-- <addr-relay-server>
//!    ```
//!
//! 2. To start the listening relay client run `cargo run --example relay -- client-listen
//! <addr-relay-server>/p2p/<peer-id-relay-server>/p2p-circuit` in a second terminal where:
//!
//!   - `<addr-relay-server>`: one of the listening addresses of the relay server
//!   - `<peer-id-relay-server>`: the peer id of the relay server
//!
//! 3. To start the dialing relay client run `cargo run --example relay -- client-dial
//! <addr-relay-server>/p2p/<peer-id-relay-server>/p2p-circuit/p2p/<peer-id-listening-relay-client>`
//! in a third terminal where:
//!
//!   - `<addr-relay-server>`: one of the listening addresses of the relay server
//!   - `<peer-id-relay-server>`: the peer id of the relay server
//!   - `<peer-id-listening-relay-client>`: the peer id of the listening relay client
//!
//! In the third terminal you will see the dialing relay client to receive pings from both the relay
//! server AND from the listening relay client relayed via the relay server.

use futures::executor::block_on;
use futures::stream::StreamExt;
use libp2p::core::upgrade;
use libp2p::ping::{Ping, PingConfig, PingEvent};
use libp2p::plaintext;
use libp2p::relay::{Relay, RelayConfig};
use libp2p::swarm::SwarmEvent;
use libp2p::tcp::TcpConfig;
use libp2p::Transport;
use libp2p::{identity, Multiaddr, NetworkBehaviour, PeerId, Swarm};
use std::error::Error;
use std::task::{Context, Poll};
use std::time::Duration;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    // Create a random PeerId
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    let tcp_transport = TcpConfig::new();

    let relay_config = RelayConfig {
        connection_idle_timeout: Duration::from_secs(10 * 60),
        ..Default::default()
    };
    let (relay_wrapped_transport, relay_behaviour) =
        libp2p_relay::new_transport_and_behaviour(relay_config, tcp_transport);

    let behaviour = Behaviour {
        relay: relay_behaviour,
        ping: Ping::new(
            PingConfig::new()
                .with_keep_alive(true)
                .with_interval(Duration::from_secs(1)),
        ),
    };

    let plaintext = plaintext::PlainText2Config {
        local_public_key: local_key.public(),
    };

    let transport = relay_wrapped_transport
        .upgrade(upgrade::Version::V1)
        .authenticate(plaintext)
        .multiplex(libp2p_yamux::YamuxConfig::default())
        .boxed();

    let mut swarm = Swarm::new(transport, behaviour, local_peer_id);

    match std::env::args()
        .nth(1)
        .expect("Please provide either of relay, client-listen or client-dial.")
        .as_str()
    {
        "relay" => {
            // Listen on all interfaces and whatever port the OS assigns
            swarm.listen_on("/ip6/::/tcp/0".parse()?)?;
        }
        "client-listen" => {
            let addr: Multiaddr = std::env::args()
                .nth(2)
                .expect("Please provide relayed listen address.")
                .parse()?;
            swarm.listen_on(addr)?;
        }
        "client-dial" => {
            let addr: Multiaddr = std::env::args()
                .nth(2)
                .expect("Please provide relayed dial address.")
                .parse()?;
            swarm.dial_addr(addr)?;
        }
        s => panic!("Unexpected argument {:?}", s),
    }

    block_on(futures::future::poll_fn(move |cx: &mut Context<'_>| {
        loop {
            match swarm.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => {
                    if let SwarmEvent::NewListenAddr(addr) = event {
                        println!("Listening on {:?}", addr);
                    }
                }
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => break,
            }
        }
        Poll::Pending
    }))
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event", event_process = false)]
struct Behaviour {
    relay: Relay,
    ping: Ping,
}

#[derive(Debug)]
enum Event {
    Relay(()),
    Ping(PingEvent),
}

impl From<PingEvent> for Event {
    fn from(e: PingEvent) -> Self {
        Event::Ping(e)
    }
}

impl From<()> for Event {
    fn from(_: ()) -> Self {
        Event::Relay(())
    }
}
