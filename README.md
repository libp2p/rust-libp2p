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

- `datastore`: Utility library whose API provides a key-value storage with multiple possible
  backends. Used by `peerstore`.
- `example`: Example usages of this library.
- `libp2p-identify`: Protocol implementation that allows a node A to query another node B what
  information B knows about A. Implements the `ConnectionUpgrade` trait of `libp2p-core`.
- `libp2p-peerstore`: Generic storage for information about remote peers (their multiaddresses and
  their public key), with multiple possible backends. Each multiaddress also has a time-to-live.
  Used by `libp2p-core`.
- `libp2p-ping`: Implementation of the `ping` protocol (the exact protocol is specific to libp2p).
  Implements the `ConnectionUpgrade` trait of `libp2p-core`.
- `libp2p-secio`: Implementation of the `secio` protocol. Encrypts communications. Implements the
  `ConnectionUpgrade` trait of `libp2p-core`.
- `libp2p-core`: Core library that contains all the traits of *libp2p* and plugs things together.
- `libp2p-tcp-transport`: Implementation of the `Transport` trait of `libp2p-core` for TCP/IP.
- `libp2p-websocket`: Implementation of the `Transport` trait of `libp2p-core` for Websockets.
- `multistream-select`: Implementation of the `multistream-select` protocol, which is used to
  negotiate a protocol over a newly-established connection with a peer, or after a connection
  upgrade.
- `rw-stream-sink`: Utility library that makes it possible to wrap around a tokio `Stream + Sink`
  of bytes and implements `AsyncRead + AsyncWrite`.

## About the `impl Trait` syntax

Right now a lot of code of this library uses `Box<Future>` or `Box<Stream>` objects, or forces
`'static` lifetime bounds.

This is caused by the lack of a stable `impl Trait` syntax in the Rust language. Once this syntax
is fully implemented and stabilized, it will be possible to change this code to use plain and
non-static objects instead of boxes.

Progress for the `impl Trait` syntax can be tracked in [this issue of the Rust repository](https://github.com/rust-lang/rust/issues/34511).

Once this syntax is stable in the nightly version, we will consider requiring the nightly version
of the compiler and switching to this syntax.

