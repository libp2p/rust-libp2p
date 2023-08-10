# Rust libp2p Server

A rust-libp2p based server implementation running:

- the [Kademlia protocol](https://github.com/libp2p/specs/tree/master/kad-dht)

- the [Circuit Relay v2 protocol](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md)

- the [AutoNAT protocol](https://github.com/libp2p/specs/blob/master/autonat/README.md)

## Usage

```
$ cargo run -- --help
libp2p server 0.5.4
A rust-libp2p server binary.

USAGE:
    libp2p-server [FLAGS] [OPTIONS]

FLAGS:
        --enable-autonat     Whether to run the libp2p Autonat protocol
        --enable-kademlia    Whether to run the libp2p Kademlia protocol and join the IPFS DHT
    -h, --help               Prints help information
    -V, --version            Prints version information

OPTIONS:
        --config <config>                Path to IPFS config file
        --metrics-path <metrics-path>    Metric endpoint path [default: /metrics]

$ cargo run
Local peer id: PeerId("12D3KooWDx8yJKVEN5LsCsovRb8HyHKA79cBshzShsE14ioS6Kok")
Listening for metric requests on 0.0.0.0:8080/metrics
Listening on "/ip4/127.0.0.1/tcp/4001"
```

