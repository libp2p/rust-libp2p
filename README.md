# Central repository for work on libp2p

<a href="http://libp2p.io/"><img src="https://img.shields.io/badge/project-libp2p-yellow.svg?style=flat-square" /></a>
[![dependency status](https://deps.rs/repo/github/libp2p/rust-libp2p/status.svg?style=flat-square)](https://deps.rs/repo/github/libp2p/rust-libp2p)
[![Crates.io](https://img.shields.io/crates/v/libp2p.svg)](https://crates.io/crates/libp2p)
[![docs.rs](https://img.shields.io/badge/api-rustdoc-blue.svg)](https://docs.rs/libp2p)

This repository is the central place for Rust development of the [libp2p](https://libp2p.io) spec.

## Getting started

- **Main documentation** can be found on https://docs.rs/libp2p.

- The **[examples](examples)** folder contains small binaries showcasing the
  many protocols in this repository.

- For **security related issues** please reach out to security@libp2p.io. Please
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

- [COMIT](https://github.com/comit-network/xmr-btc-swap) - Bitcoin–Monero Cross-chain Atomic Swap.
- [Forest](https://github.com/ChainSafe/forest) - An implementation of Filecoin written in Rust.
- [fuel-core](https://github.com/FuelLabs/fuel-core) - A Rust implementation of the Fuel protocol.
- [ipfs-embed](https://github.com/ipfs-rust/ipfs-embed) - A small embeddable ipfs implementation
used and maintained by [Actyx][https://www.actyx.com].
- [iroh](https://github.com/n0-computer/iroh) - Next-generation implementation of IPFS for Cloud & Mobile platforms.
- [Lighthouse](https://github.com/sigp/lighthouse) - Ethereum consensus client in Rust.
- [Locutus](https://github.com/freenet/locutus) - Global, observable, decentralized key-value store.
- [rust-ipfs](https://github.com/rs-ipfs/rust-ipfs) - IPFS implementation in Rust.
- [Starcoin](https://github.com/starcoinorg/starcoin) - A smart contract blockchain network that scales by layering.
- [Subspace](https://github.com/subspace/subspace) - Subspace Network reference implementation
- [Substrate](https://github.com/paritytech/substrate) - Framework for blockchain innovation,
used by [Polkadot](https://www.parity.io/technologies/polkadot/).
