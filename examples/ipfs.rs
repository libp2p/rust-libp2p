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

//! A basic chat application demonstrating libp2p and the mDNS and floodsub protocols.
//!
//! Using two terminal windows, start two instances. If you local network allows mDNS,
//! they will automatically connect. Type a message in either terminal and hit return: the
//! message is sent and printed in the other terminal. Close with Ctrl-c.
//!
//! You can of course open more terminal windows and add more participants.
//! Dialing any of the other peers will propagate the new participant to all
//! chat members and everyone will receive all messages.
//!
//! # If they don't automatically connect
//!
//! If the nodes don't automatically connect, take note of the listening address of the first
//! instance and start the second with this address as the first argument. In the first terminal
//! window, run:
//!
//! ```sh
//! cargo run --example chat
//! ```
//!
//! It will print the PeerId and the listening address, e.g. `Listening on
//! "/ip4/0.0.0.0/tcp/24915"`
//!
//! In the second terminal window, start a new instance of the example with:
//!
//! ```sh
//! cargo run --example chat -- /ip4/127.0.0.1/tcp/24915
//! ```
//!
//! The two nodes then connect.

use async_std::{io, task};
use futures::{future, prelude::*};
use libp2p::core::transport::upgrade::Version;
use libp2p::identify::{Identify, IdentifyEvent};
use libp2p::pnet::PnetConfig;
use libp2p::secio::SecioConfig;
use libp2p::tcp::TcpConfig;
use libp2p::yamux::Config as YamuxConfig;
use libp2p::Transport;
use libp2p::TransportError;
use libp2p::{
    gossipsub::{self, Gossipsub, GossipsubConfigBuilder, GossipsubEvent},
    identity,
    ping::{self, Ping, PingConfig, PingEvent},
    pnet::PreSharedKey,
    swarm::NetworkBehaviourEventProcess,
    Multiaddr, NetworkBehaviour, PeerId, Swarm,
};
use libp2p_core::either::{EitherError, EitherFuture, EitherListenStream, EitherOutput};
use libp2p_core::StreamMuxer;
use std::str::FromStr;
use std::time::Duration;
use std::{
    error::Error,
    task::{Context, Poll},
};

/// an XOR combination of two transports
#[derive(Debug, Copy, Clone)]
pub enum EitherTransport<A, B> {
    Left(A),
    Right(B),
}

impl<A, B> Transport for EitherTransport<A, B>
where
    B: Transport,
    A: Transport,
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = EitherError<A::Error, B::Error>;
    type Listener = EitherListenStream<A::Listener, B::Listener>;
    type ListenerUpgrade = EitherFuture<A::ListenerUpgrade, B::ListenerUpgrade>;
    type Dial = EitherFuture<A::Dial, B::Dial>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let addr = match self {
            EitherTransport::Left(a) => match a.listen_on(addr) {
                Ok(listener) => return Ok(EitherListenStream::First(listener)),
                Err(TransportError::MultiaddrNotSupported(addr)) => addr,
                Err(TransportError::Other(err)) => {
                    return Err(TransportError::Other(EitherError::A(err)))
                }
            },
            EitherTransport::Right(b) => match b.listen_on(addr) {
                Ok(listener) => return Ok(EitherListenStream::Second(listener)),
                Err(TransportError::MultiaddrNotSupported(addr)) => addr,
                Err(TransportError::Other(err)) => {
                    return Err(TransportError::Other(EitherError::B(err)))
                }
            },
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let addr = match self {
            EitherTransport::Left(a) => match a.dial(addr) {
                Ok(connec) => return Ok(EitherFuture::First(connec)),
                Err(TransportError::MultiaddrNotSupported(addr)) => addr,
                Err(TransportError::Other(err)) => {
                    return Err(TransportError::Other(EitherError::A(err)))
                }
            },
            EitherTransport::Right(b) => match b.dial(addr) {
                Ok(connec) => return Ok(EitherFuture::Second(connec)),
                Err(TransportError::MultiaddrNotSupported(addr)) => addr,
                Err(TransportError::Other(err)) => {
                    return Err(TransportError::Other(EitherError::B(err)))
                }
            },
        };

        Err(TransportError::MultiaddrNotSupported(addr))
    }
}

/// Builds the transport that serves as a common ground for all connections.
pub fn build_transport(
    key_pair: identity::Keypair,
    psk: Option<PreSharedKey>,
) -> impl Transport<
    Output = (
        PeerId,
        impl StreamMuxer<
                OutboundSubstream = impl Send,
                Substream = impl Send,
                Error = impl Into<io::Error>,
            > + Send
            + Sync,
    ),
    Error = impl Error + Send,
    Listener = impl Send,
    Dial = impl Send,
    ListenerUpgrade = impl Send,
> + Clone {
    let secio_config = SecioConfig::new(key_pair);
    let yamux_config = YamuxConfig::default();

    let base_transport = TcpConfig::new().nodelay(true);
    let maybe_encrypted = match psk {
        Some(psk) => EitherTransport::Left(
            base_transport.and_then(move |socket, _| PnetConfig::new(psk).handshake(socket)),
        ),
        None => EitherTransport::Right(base_transport),
    };
    maybe_encrypted
        .upgrade(Version::V1)
        .authenticate(secio_config)
        .multiplex(yamux_config)
        .timeout(Duration::from_secs(20))
}

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

fn get_psk(path: Box<Path>) -> std::io::Result<Option<String>> {
    let swarm_key_file = path.join("swarm.key");
    match fs::read_to_string(swarm_key_file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

use std::{env, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let ipfs_path: Box<Path> = get_ipfs_path();
    println!("using IPFS_PATH {:?}", ipfs_path);
    let psk: Option<PreSharedKey> = get_psk(ipfs_path)?.map(|text| PreSharedKey::from_str(&text)).transpose()?;

    // Create a random PeerId
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    // let psk = PreSharedKey::from_str("/key/swarm/psk/1.0.0/\n/base16/\n6189c5cf0b87fb800c1a9feeda73c6ab5e998db48fb9e6a978575c770ceef683").unwrap();
    println!("using random peer id: {:?}", local_peer_id);
    for psk in psk {
        println!("using swarm key with fingerprint: {}", psk.fingerprint());
    }

    // Set up a an encrypted DNS-enabled TCP Transport over the Mplex and Yamux protocols
    let transport = build_transport(local_key.clone(), psk);

    // Create a Floodsub topic
    let gossipsub_topic = gossipsub::Topic::new("chat".into());

    // We create a custom network behaviour that combines floodsub and mDNS.
    // In the future, we want to improve libp2p to make this easier to do.
    // Use the derive to generate delegating NetworkBehaviour impl and require the
    // NetworkBehaviourEventProcess implementations below.
    #[derive(NetworkBehaviour)]
    struct MyBehaviour<TSubstream: AsyncRead + AsyncWrite> {
        gossipsub: Gossipsub<TSubstream>,
        identify: Identify<TSubstream>,
        ping: Ping<TSubstream>,
    }

    impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<IdentifyEvent>
        for MyBehaviour<TSubstream>
    {
        // Called when `identify` produces an event.
        fn inject_event(&mut self, event: IdentifyEvent) {
            println!("identify: {:?}", event);
        }
    }

    impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<GossipsubEvent>
        for MyBehaviour<TSubstream>
    {
        // Called when `mdns` produces an event.
        fn inject_event(&mut self, event: GossipsubEvent) {
            match event {
                GossipsubEvent::Message(peer_id, id, message) => println!(
                    "Got message: {} with id: {} from peer: {:?}",
                    String::from_utf8_lossy(&message.data),
                    id,
                    peer_id
                ),
                _ => {}
            }
        }
    }

    impl<TSubstream: AsyncRead + AsyncWrite> NetworkBehaviourEventProcess<PingEvent>
        for MyBehaviour<TSubstream>
    {
        // Called when `ping` produces an event.
        fn inject_event(&mut self, event: PingEvent) {
            use ping::handler::{PingFailure, PingSuccess};
            match event {
                PingEvent {
                    peer,
                    result: Result::Ok(PingSuccess::Ping { rtt }),
                } => {
                    println!(
                        "ping: rtt to {} is {} ms",
                        peer.to_base58(),
                        rtt.as_millis()
                    );
                }
                PingEvent {
                    peer,
                    result: Result::Ok(PingSuccess::Pong),
                } => {
                    println!("ping: pong from {}", peer.to_base58());
                }
                PingEvent {
                    peer,
                    result: Result::Err(PingFailure::Timeout),
                } => {
                    println!("ping: timeout to {}", peer.to_base58());
                }
                PingEvent {
                    peer,
                    result: Result::Err(PingFailure::Other { error }),
                } => {
                    println!("ping: failure with {}: {}", peer.to_base58(), error);
                }
            }
        }
    }

    // Create a Swarm to manage peers and events
    let mut swarm = {
        let gossipsub_config = GossipsubConfigBuilder::default()
            .max_transmit_size(32768)
            .build();
        let mut behaviour = MyBehaviour {
            gossipsub: Gossipsub::new(local_peer_id.clone(), gossipsub_config),
            identify: Identify::new("/ipfs/0.1.0".into(), "rust-ipfs".into(), local_key.public()),
            ping: Ping::new(PingConfig::new()),
        };

        println!("Subscribing to {:?}", gossipsub_topic);
        behaviour.gossipsub.subscribe(gossipsub_topic.clone());
        Swarm::new(transport, behaviour, local_peer_id.clone())
    };

    // Reach out to another node if specified
    if let Some(to_dial) = std::env::args().nth(1) {
        let addr: Multiaddr = to_dial.parse()?;
        Swarm::dial_addr(&mut swarm, addr)?;
        println!("Dialed {:?}", to_dial)
    }

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    // Listen on all interfaces and whatever port the OS assigns
    Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse()?)?;

    // Kick it off
    let mut listening = false;
    task::block_on(future::poll_fn(move |cx: &mut Context| {
        loop {
            match stdin.try_poll_next_unpin(cx)? {
                Poll::Ready(Some(line)) => {
                    swarm.gossipsub.publish(&gossipsub_topic, line.as_bytes());
                }
                Poll::Ready(None) => panic!("Stdin closed"),
                Poll::Pending => break,
            }
        }
        loop {
            match swarm.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => println!("{:?}", event),
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => {
                    if !listening {
                        for addr in Swarm::listeners(&swarm) {
                            println!("Identity {}/ipfs/{}", addr, local_peer_id);
                            listening = true;
                        }
                    }
                    break;
                }
            }
        }
        Poll::Pending
    }))
}
