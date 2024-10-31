# Rust libp2p Server

A rust-libp2p based server implementation running:

- the [Kademlia protocol](https://github.com/libp2p/specs/tree/master/kad-dht)

- the [Circuit Relay v2 protocol](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md)

- the [AutoNAT protocol](https://github.com/libp2p/specs/blob/master/autonat/README.md)

## Usage

```
cargo run -- --help

A rust-libp2p server binary.

Usage: libp2p-server [OPTIONS] --config <CONFIG>

Options:
      --config <CONFIG>              Path to IPFS config file
      --metrics-path <METRICS_PATH>  Metric endpoint path [default: /metrics]
      --enable-kademlia              Whether to run the libp2p Kademlia protocol and join the IPFS DHT
      --enable-autonat               Whether to run the libp2p Autonat protocol
  -h, --help                         Print help
```

```
cargo run -- --config ~/.ipfs/config

Local peer id: PeerId("12D3KooWSa1YEeQVSwvoqAMhwjKQ6kqZQckhWPb3RWEGV3sZGU6Z")
Listening on "/ip4/127.0.0.1/udp/4001/quic"
[...]
```
