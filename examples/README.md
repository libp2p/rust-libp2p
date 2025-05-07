# Examples

A set of examples showcasing how to use rust-libp2p.

## Getting started

To run any example in this directory, you can use Cargo:

```sh
# Navigate to the specific example directory
cd examples/ping

# Run the example
cargo run
```

Each example includes its own README.md file with specific instructions on how to use it. Most examples require running multiple instances to demonstrate peer-to-peer communication, so be sure to read the individual example documentation.

### Prerequisites

- Rust and Cargo installed (see [rustup.rs](https://rustup.rs/) for installation)
- Basic understanding of peer-to-peer networking concepts
- Some examples may require additional dependencies specific to their functionality

## Individual libp2p features

- [Chat](./chat) A basic chat application demonstrating libp2p and the mDNS and Gossipsub protocols.
- [Distributed key-value store](./distributed-key-value-store) A basic key value store demonstrating libp2p and the mDNS and Kademlia protocol.

- [File sharing application](./file-sharing) Basic file sharing application with peers either providing or locating and getting files by name.

  While obviously showcasing how to build a basic file sharing application with the Kademlia and
  Request-Response protocol, the actual goal of this example is **to show how to integrate
  rust-libp2p into a larger application**.

- [IPFS Kademlia](./ipfs-kad) Demonstrates how to perform Kademlia queries on the IPFS network.

- [IPFS Private](./ipfs-private) Implementation using the gossipsub, ping and identify protocols to implement the ipfs private swarms feature.

- [Ping](./ping) Small `ping` clone, sending a ping to a peer, expecting a pong as a response. See [tutorial](../libp2p/src/tutorials/ping.rs) for a step-by-step guide building the example.

- [Rendezvous](./rendezvous) Rendezvous Protocol. See [specs](https://github.com/libp2p/specs/blob/master/rendezvous/README.md).
