# Central repository for work on libp2p

<a href="http://libp2p.io/"><img src="https://img.shields.io/badge/project-libp2p-yellow.svg?style=flat-square" /></a>
[![dependency status](https://deps.rs/repo/github/libp2p/rust-libp2p/status.svg?style=flat-square)](https://deps.rs/repo/github/libp2p/rust-libp2p)

This repository is the central place for Rust development of the [libp2p](https://libp2p.io) spec.

## Getting started

- **Main documentation** can be found on https://docs.rs/libp2p.

- The **[examples](examples)** folder contains small binaries showcasing the
  many protocols in this repository.

- For **security related issues** please reach out to security@ipfs.io. Please
  do not file a public issue on GitHub.

- To **report bugs, suggest improvements or request new features** please open a
  GitHub issue on this repository.

- For **rust-libp2p specific questions** please use the GitHub _Discussions_
  forum https://github.com/libp2p/rust-libp2p/discussions.

- For **discussions and questions related to multiple libp2p implementations**
  please use the libp2p _Discourse_ forum https://discuss.libp2p.io.

- For general project updates and discussions join the [biweekly libp2p Community
  Calls](https://discuss.libp2p.io/t/libp2p-community-calls/1157).

## Repository Structure

The main components of this repository are structured as follows:

  * `core/`: The implementation of `libp2p-core` with its `Transport` and
    `StreamMuxer` API on which almost all other crates depend.

  * `transports/`: Implementations of transport protocols (e.g. TCP) and protocol upgrades
    (e.g. for authenticated encryption, compression, ...) based on the `libp2p-core` `Transport`
    API .

  * `muxers/`: Implementations of the `StreamMuxer` interface of `libp2p-core`,
    e.g. (sub)stream multiplexing protocols on top of (typically TCP) connections.
    Multiplexing protocols are (mandatory) `Transport` upgrades.

  * `swarm/`: The implementation of `libp2p-swarm` building on `libp2p-core`
    with the central interfaces `NetworkBehaviour` and `ConnectionHandler` used
    to implement application protocols (see `protocols/`).

  * `protocols/`: Implementations of application protocols based on the
    `libp2p-swarm` APIs.

  * `misc/`: Utility libraries.

  * `examples/`: Worked examples of built-in application protocols (see `protocols/`)
    with common `Transport` configurations.

## Community Guidelines

The libp2p project operates under the [IPFS Code of
Conduct](https://github.com/ipfs/community/blob/master/code-of-conduct.md).

> tl;dr
>
> - Be respectful.
> - We're here to help: abuse@ipfs.io
> - Abusive behavior is never tolerated.
> - Violations of this code may result in swift and permanent expulsion from the
>   IPFS [and libp2p] community.
> - "Too long, didn't read" is not a valid excuse for not knowing what is in
>   this document.

## Maintainers

(In alphabetical order.)

- Elena Frank ([@elenaf9](https://github.com/elenaf9/))
- Max Inden ([@mxinden](https://github.com/mxinden/))
- Thomas Eizinger ([@thomaseizinger](https://github.com/thomaseizinger))

## Notable users

(open a pull request if you want your project to be added here)

- https://github.com/paritytech/polkadot
- https://github.com/paritytech/substrate
- https://github.com/sigp/lighthouse
- https://github.com/golemfactory/golem-libp2p
- https://github.com/comit-network
- https://github.com/rs-ipfs/rust-ipfs
- https://github.com/marcopoloprotocol/marcopolo
- https://github.com/ChainSafe/forest
- https://github.com/ipfs-rust/ipfs-embed
- https://www.actyx.com/developers/
- https://github.com/starcoinorg/starcoin
