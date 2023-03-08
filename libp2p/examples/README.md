# Examples

A set of examples showcasing how to use rust-libp2p.

## Getting started


## Individual libp2p features

- [Chat](/examples/chat-example) A basic chat application demonstrating libp2p and the mDNS and Gossipsub protocols.
- [Distributed key-value store](/examples/distributed-key-value-store) A basic key value store demonstrating libp2p and the mDNS and Kademlia protocol.

- [File sharing application](/examples/file-sharing) Basic file sharing application with peers either providing or locating and getting files by name.

  While obviously showcasing how to build a basic file sharing application with the Kademlia and
  Request-Response protocol, the actual goal of this example is **to show how to integrate
  rust-libp2p into a larger application**.

- [IPFS Kademlia](/examples/ipfs-kad) Demonstrates how to perform Kademlia queries on the IPFS network.

- [IPFS Private](/examples/ipfs-private) Implementation using the gossipsub, ping and identify protocols to implement the ipfs private swarms feature.

- [Ping](/examples/ping-example) Small `ping` clone, sending a ping to a peer, expecting a pong as a response. See [tutorial](../src/tutorials/ping.rs) for a step-by-step guide building the example.


- [Rendezvous](/examples/rendezvous) Rendezvous Protocol. See [specs](https://github.com/libp2p/specs/blob/master/rendezvous/README.md).

