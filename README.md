# [WIP] Central repository for work on libp2p

This repository is the central place for rust development of the
[libp2p](https://libp2p.io) spec.

This readme along with many others will be more fleshed out the closer
the project gets to completion. Right now everything including the crate
organization is very much Work in Progress.

## General overview of the architecture

Architecture of the crates of this repository:

- `datastore`: Utility library whose API provides a key-value storage with multiple possible
  backends.
- `libp2p-host`: Stub. Will probably get reworked or removed.
- `libp2p-tcp-transport`: Implementation of the `Transport` trait for TCP/IP.
- `libp2p-transport`: Contains the `Transport` trait. Will probably get reworked or removed.
- `multistream-select`: Implementation of the `multistream-select` protocol, which is used to
  negotiate a protocol over a newly-established connection with a peer, or after a connection
  upgrade.
