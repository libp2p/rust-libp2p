// Copyright 2021 Protocol Labs.
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

//! # Ping Tutorial - Getting started with rust-libp2p
//!
//! This tutorial aims to give newcomers a hands-on overview of how to use the
//! Rust libp2p implementation. People new to Rust likely want to get started on
//! [Rust](https://www.rust-lang.org/) itself, before diving into all the
//! networking fun. This library makes heavy use of asynchronous Rust. In case
//! you are not familiar with this concept, the Rust
//! [async-book](https://rust-lang.github.io/async-book/) should prove useful.
//! People new to libp2p might prefer to get a general overview at
//! [libp2p.io](https://libp2p.io/)
//! first, although libp2p knowledge is not required for this tutorial.
//!
//! We are going to build a small `ping` clone, sending a ping to a peer,
//! expecting a pong as a response.
//!
//! ## Scaffolding
//!
//! Let's start off by
//!
//! 1. Updating to the latest Rust toolchain, e.g.: `rustup update`
//!
//! 2. Creating a new crate: `cargo init rust-libp2p-tutorial`
//!
//! 3. Adding `libp2p` as well as `futures` as dependencies in the
//!    `Cargo.toml` file. Current crate versions may be found at
//!    [crates.io](https://crates.io/).
//!    We will also include `tokio` with the
//!    "attributes" feature to allow for an `async main`.
//!    At the time of writing we have:
//!
//!    ```yaml
//!    [package]
//!        name = "rust-libp2p-tutorial"
//!        version = "0.1.0"
//!        edition = "2021"
//!
//!    [dependencies]
//!        libp2p = { version = "0.54", features = ["noise", "ping", "tcp", "tokio", "yamux"] }
//!        futures = "0.3.30"
//!        tokio = { version = "1.37.0", features = ["full"] }
//!        tracing-subscriber = { version = "0.3", features = ["env-filter"] }
//!    ```
//!
//! ## Network identity
//!
//! With all the scaffolding in place, we can dive into the libp2p specifics.
//! First we need to create a network identity for our local node in `async fn
//! main()`, annotated with an attribute to allow `main` to be `async`.
//! Identities in libp2p are handled via a public/private key pair.
//! Nodes identify each other via their [`PeerId`](crate::PeerId) which is
//! derived from their public key. Now, replace the contents of main.rs by:
//!
//! ```rust
//! use std::error::Error;
//!
//! use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let _ = tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .try_init();
//!
//!     let mut swarm = libp2p::SwarmBuilder::with_new_identity();
//!
//!     Ok(())
//! }
//! ```
//!
//! Go ahead and build and run the above code with: `cargo run`. Nothing happening thus far.
//!
//! ## Transport
//!
//! Next up we need to construct a transport. Each transport in libp2p provides encrypted streams.
//! E.g. combining TCP to establish connections, NOISE to encrypt these connections and Yamux to run
//! one or more streams on a connection. Another libp2p transport is QUIC, providing encrypted
//! streams out-of-the-box. We will stick to TCP for now. Each of these implement the [`Transport`]
//! trait.
//!
//! ```rust
//! use std::error::Error;
//!
//! use libp2p::{noise, tcp, yamux};
//! use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let _ = tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .try_init();
//!
//!     let mut swarm = libp2p::SwarmBuilder::with_new_identity()
//!         .with_tokio()
//!         .with_tcp(
//!             tcp::Config::default(),
//!             noise::Config::new,
//!             yamux::Config::default,
//!         )?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Network behaviour
//!
//! Now it is time to look at another core trait of rust-libp2p: the
//! [`NetworkBehaviour`]. While the previously introduced trait [`Transport`]
//! defines _how_ to send bytes on the network, a [`NetworkBehaviour`] defines
//! _what_ bytes and to _whom_ to send on the network.
//!
//! To make this more concrete, let's take a look at a simple implementation of
//! the [`NetworkBehaviour`] trait: the [`ping::Behaviour`](crate::ping::Behaviour).
//! As you might have guessed, similar to the good old ICMP `ping` network tool,
//! libp2p [`ping::Behaviour`](crate::ping::Behaviour) sends a ping to a peer and expects
//! to receive a pong in turn. The [`ping::Behaviour`](crate::ping::Behaviour) does not care _how_
//! the ping and pong messages are sent on the network, whether they are sent via
//! TCP, whether they are encrypted via [noise](crate::noise) or just in
//! [plaintext](crate::plaintext). It only cares about _what_ messages and to _whom_ to sent on the
//! network.
//!
//! The two traits [`Transport`] and [`NetworkBehaviour`] allow us to cleanly
//! separate _how_ to send bytes from _what_ bytes and to _whom_ to send.
//!
//! With the above in mind, let's extend our example, creating a
//! [`ping::Behaviour`](crate::ping::Behaviour) at the end:
//!
//! ```rust
//! use std::error::Error;
//!
//! use libp2p::{noise, ping, tcp, yamux};
//! use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let _ = tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .try_init();
//!
//!     let mut swarm = libp2p::SwarmBuilder::with_new_identity()
//!         .with_tokio()
//!         .with_tcp(
//!             tcp::Config::default(),
//!             noise::Config::new,
//!             yamux::Config::default,
//!         )?
//!         .with_behaviour(|_| ping::Behaviour::default())?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Swarm
//!
//! Now that we have a [`Transport`] and a [`NetworkBehaviour`], we can build the [`Swarm`]
//! which connects the two, allowing both to make progress. Put simply, a [`Swarm`] drives both a
//! [`Transport`] and a [`NetworkBehaviour`] forward, passing commands from the [`NetworkBehaviour`]
//! to the [`Transport`] as well as events from the [`Transport`] to the [`NetworkBehaviour`].
//!
//! ```rust
//! use std::error::Error;
//!
//! use libp2p::{noise, ping, tcp, yamux};
//! use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let _ = tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .try_init();
//!
//!     let mut swarm = libp2p::SwarmBuilder::with_new_identity()
//!         .with_tokio()
//!         .with_tcp(
//!             tcp::Config::default(),
//!             noise::Config::new,
//!             yamux::Config::default,
//!         )?
//!         .with_behaviour(|_| ping::Behaviour::default())?
//!         .build();
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Idle connection timeout
//!
//! Now, for this example in particular, we need set the idle connection timeout.
//! Otherwise, the connection will be closed immediately.
//!
//! Whether you need to set this in your application too depends on your usecase.
//! Typically, connections are kept alive if they are "in use" by a certain protocol.
//! The ping protocol however is only an "auxiliary" kind of protocol.
//! Thus, without any other behaviour in place, we would not be able to observe the pings.
//!
//! ```rust
//! use std::{error::Error, time::Duration};
//!
//! use libp2p::{noise, ping, tcp, yamux};
//! use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let _ = tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .try_init();
//!
//!     let mut swarm = libp2p::SwarmBuilder::with_new_identity()
//!         .with_tokio()
//!         .with_tcp(
//!             tcp::Config::default(),
//!             noise::Config::new,
//!             yamux::Config::default,
//!         )?
//!         .with_behaviour(|_| ping::Behaviour::default())?
//!         .build();
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Multiaddr
//!
//! With the [`Swarm`] in place, we are all set to listen for incoming
//! connections. We only need to pass an address to the [`Swarm`], just like for
//! [`std::net::TcpListener::bind`]. But instead of passing an IP address, we
//! pass a [`Multiaddr`] which is yet another core concept of libp2p worth
//! taking a look at.
//!
//! A [`Multiaddr`] is a self-describing network address and protocol stack that
//! is used to establish connections to peers. A good introduction to
//! [`Multiaddr`] can be found at
//! [docs.libp2p.io/concepts/addressing](https://docs.libp2p.io/concepts/addressing/)
//! and its specification repository
//! [github.com/multiformats/multiaddr](https://github.com/multiformats/multiaddr/).
//!
//! Let's make our local node listen on a new socket.
//! This socket is listening on multiple network interfaces at the same time. For
//! each network interface, a new listening address is created. These may change
//! over time as interfaces become available or unavailable.
//! For example, in case of our TCP transport it may (among others) listen on the
//! loopback interface (localhost) `/ip4/127.0.0.1/tcp/24915` as well as the local
//! network `/ip4/192.168.178.25/tcp/24915`.
//!
//! In addition, if provided on the CLI, let's instruct our local node to dial a
//! remote peer.
//!
//! ```rust
//! use std::{error::Error, time::Duration};
//!
//! use libp2p::{noise, ping, tcp, yamux, Multiaddr};
//! use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let _ = tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .try_init();
//!
//!     let mut swarm = libp2p::SwarmBuilder::with_new_identity()
//!         .with_tokio()
//!         .with_tcp(
//!             tcp::Config::default(),
//!             noise::Config::new,
//!             yamux::Config::default,
//!         )?
//!         .with_behaviour(|_| ping::Behaviour::default())?
//!         .build();
//!
//!     // Tell the swarm to listen on all interfaces and a random, OS-assigned
//!     // port.
//!     swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
//!
//!     // Dial the peer identified by the multi-address given as the second
//!     // command-line argument, if any.
//!     if let Some(addr) = std::env::args().nth(1) {
//!         let remote: Multiaddr = addr.parse()?;
//!         swarm.dial(remote)?;
//!         println!("Dialed {addr}")
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Continuously polling the Swarm
//!
//! We have everything in place now. The last step is to drive the [`Swarm`] in
//! a loop, allowing it to listen for incoming connections and establish an
//! outgoing connection in case we specify an address on the CLI.
//!
//! ```no_run
//! use std::{error::Error, time::Duration};
//!
//! use futures::prelude::*;
//! use libp2p::{noise, ping, swarm::SwarmEvent, tcp, yamux, Multiaddr};
//! use tracing_subscriber::EnvFilter;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn Error>> {
//!     let _ = tracing_subscriber::fmt()
//!         .with_env_filter(EnvFilter::from_default_env())
//!         .try_init();
//!
//!     let mut swarm = libp2p::SwarmBuilder::with_new_identity()
//!         .with_tokio()
//!         .with_tcp(
//!             tcp::Config::default(),
//!             noise::Config::new,
//!             yamux::Config::default,
//!         )?
//!         .with_behaviour(|_| ping::Behaviour::default())?
//!         .build();
//!
//!     // Tell the swarm to listen on all interfaces and a random, OS-assigned
//!     // port.
//!     swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
//!
//!     // Dial the peer identified by the multi-address given as the second
//!     // command-line argument, if any.
//!     if let Some(addr) = std::env::args().nth(1) {
//!         let remote: Multiaddr = addr.parse()?;
//!         swarm.dial(remote)?;
//!         println!("Dialed {addr}")
//!     }
//!
//!     loop {
//!         match swarm.select_next_some().await {
//!             SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {address:?}"),
//!             SwarmEvent::Behaviour(event) => println!("{event:?}"),
//!             _ => {}
//!         }
//!     }
//! }
//! ```
//!
//! ## Running two nodes
//!
//! For convenience the example created above is also implemented in full in
//! `examples/ping.rs`. Thus, you can either run the commands below from your
//! own project created during the tutorial, or from the root of the rust-libp2p
//! repository. Note that in the former case you need to ignore the `--example
//! ping` argument.
//!
//! You need two terminals. In the first terminal window run:
//!
//! ```sh
//! cargo run --example ping
//! ```
//!
//! It will print the new listening addresses, e.g.
//! ```sh
//! Listening on "/ip4/127.0.0.1/tcp/24915"
//! Listening on "/ip4/192.168.178.25/tcp/24915"
//! Listening on "/ip4/172.17.0.1/tcp/24915"
//! Listening on "/ip6/::1/tcp/24915"
//! ```
//!
//! In the second terminal window, start a new instance of the example with:
//!
//! ```sh
//! cargo run --example ping -- /ip4/127.0.0.1/tcp/24915
//! ```
//!
//! Note: The [`Multiaddr`] at the end being one of the [`Multiaddr`] printed
//! earlier in terminal window one.
//! Both peers have to be in the same network with which the address is associated.
//! In our case any printed addresses can be used, as both peers run on the same
//! device.
//!
//! The two nodes will establish a connection and send each other ping and pong
//! messages every 15 seconds.
//!
//! [`Multiaddr`]: crate::core::Multiaddr
//! [`NetworkBehaviour`]: crate::swarm::NetworkBehaviour
//! [`Transport`]: crate::core::Transport
//! [`PeerId`]: crate::core::PeerId
//! [`Swarm`]: crate::swarm::Swarm
