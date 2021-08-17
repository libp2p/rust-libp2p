# Examples

A set of examples showcasing how to use rust-libp2p.

## Getting started

- [Ping](ping.rs)

  Small `ping` clone, sending a ping to a peer, expecting a pong as a response. See
  [tutorial](../src/tutorial.rs) for a step-by-step guide building the example.

## Individual libp2p protocols

- [Chat](./chat.rs)

   A basic chat application demonstrating libp2p and the mDNS and floodsub protocols.

    - [Gossipsub chat](./gossipsub-chat.rs)

      Same as the chat example but using the Gossipsub protocol.

    - [Tokio based chat](./chat-tokio.rs)

      Same as the chat example but using tokio for all asynchronous tasks and I/O.

- [Distributed key-value store](./distributed-key-value-store.rs)

  A basic key value store demonstrating libp2p and the mDNS and Kademlia protocol.

- [IPFS Kademlia](ipfs-kad.rs)

  Demonstrates how to perform Kademlia queries on the IPFS network.

- [IPFS Private](ipfs-private.rs)

  Implementation using the gossipsub, ping and identify protocols to implement the ipfs private
  swarms feature.

- [Passive Discovery via MDNS](mdns-passive-discovery.rs)

  Discover peers on the same network via the MDNS protocol.

## Integration into a larger application

- [File sharing application](./file-sharing.rs)

  Basic file sharing application with peers either providing or locating and getting files by name.

  While obviously showcasing how to build a basic file sharing application with the Kademlia and
  Request-Response protocol, the actual goal of this example is **to show how to integrate
  rust-libp2p into a larger application**.
