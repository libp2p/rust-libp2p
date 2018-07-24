# [WIP] Central repository for work on libp2p

This repository is the central place for rust development of the
[libp2p](https://libp2p.io) spec.

This readme along with many others will be more fleshed out the closer
the project gets to completion. Right now everything including the crate
organization is very much Work in Progress.

## The main crate: libp2p

This repository includes a facade crate named `libp2p`, which reexports the rest of the repository.

## General overview of the architecture

Architecture of the other crates of this repository:

- `cid`: implements [CID (Content IDentifier)](https://github.com/ipld/cid): Self-describing content-addressed identifiers for distributed systems.
- `circular-buffer`: An optimized FIFO queue that allows safe access to the internal storage as a slice (i.e. not just element-by-element). This is useful for circular buffers of bytes. Since it uses `smallvec`'s `Array` trait it can only be backed by an array of static size, this may change in the future.
- `core`: Transport, protocol upgrade and swarm systems of libp2p. This crate contains all the core traits and mechanisms of the transport and swarm systems of libp2p.
- `datastore`: Utility library whose API provides a key-value storage with multiple possible backends. Used by `peerstore`.
- `dns`: this crate provides the type `DnsConfig` that allows one to resolve the `/dns4/` and `/dns6/` components of multiaddresses.
- `example`: Example usages of this library.
- `floodsub`: a flooding PubSub implementation for p2p messaging.
- `identify`: implementation of the `/ipfs/id/1.0.0`<!--TODO: where is this? It's also stated in multicodec but I can't find the source code.--> protocol that allows a node A to query another node B what information B knows about A. Implements the `ConnectionUpgrade` trait of `core`. Also includes the addresses B is listening on.
- `kad`: kademlia DHT implementation for peer routing
- `mplex`: Implements a binary stream [multiplex](https://github.com/maxogden/multiplex)er, streams multiple streams of binary data over a single binary stream.
- [`multiaddr`](https://github.com/multiformats/multiaddr): composable, future-proof, efficient and self-describing network addresses
- [`multihash`](https://github.com/multiformats/multihash): self identifying hashes; differentiating outputs from various well-established cryptographic hash functions; addressing size + encoding considerations; future-proofing use of hashes, and allowing multiple hash functions to coexist.
- `multistream-select`: used internally by libp2p to negotiate a protocol over a newly-established connection with a peer, or after a connection upgrade.
- `peerstore`: Generic storage for information about remote peers (their multiaddresses and their public key), with multiple possible backends. Each multiaddress also has a time-to-live. Used by `core`.
- `ping`: Implementation of the `ping` protocol (the exact protocol is specific to libp2p). Implements the `ConnectionUpgrade` trait of `core`.
- `ratelimit`: manages rate limiting with a connection. Also see [here](https://github.com/libp2p/specs/blob/master/8-implementations.md#811-swarm-dialer) for design.
- `relay`: Implements the [`/libp2p/circuit/relay/0.1.0` protocol](https://github.com/libp2p/specs/blob/master/relay/). It allows a source `A` to connect to a destination `B` via an intermediate relay node `R` to which `B` is already connected to. This is used as a last resort to make `B` reachable from other nodes when it would normally not be, e.g. due to certain NAT setups.
- `rw-stream-sink`: Utility library that makes it possible to wrap around a tokio `Stream + Sink` of bytes and implements `AsyncRead + AsyncWrite`.
- `secio`: Implementation of the `secio` protocol. Encrypts communications. Implements the `ConnectionUpgrade` trait of `core`.
- `tcp-transport`: Implementation of the `Transport` trait of `core` for TCP/IP.
- `varint`: encoding and decoding state machines for protobuf varints.
- `websocket`: Implementation of the `Transport` trait of `core` for Websockets.

## About the `impl Trait` syntax

Right now a lot of code of this library uses `Box<Future>` or `Box<Stream>` objects, or forces
`'static` lifetime bounds.

This is caused by the lack of a stable `impl Trait` syntax in the Rust language. Once this syntax
is fully implemented and stabilized, it will be possible to change this code to use plain and
non-static objects instead of boxes.

Progress for the `impl Trait` syntax can be tracked in [this issue of the Rust repository](https://github.com/rust-lang/rust/issues/34511).

Once this syntax is stable in the nightly version, we will consider requiring the nightly version
of the compiler and switching to this syntax.

