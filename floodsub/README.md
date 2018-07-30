# Floodsub

> A flooding PubSub system for p2p messaging.

PubSub is a work in progress, with [`floodsub`](https://github.com/libp2p/go-floodsub/) as an initial protocol. Research on `gossipsub` (which is implemented in Go [here](https://github.com/libp2p/go-floodsub/pull/67)) is underway and work on it for Rust is planned to be started soon. Floodsub broadcasts a message to all peers and subscribes to all peers for messages of a topic. In gossipsub, a peer maintains a list of other peers to dial and listen to (as a host) messages for a particular topic.

For the specification, see [here](https://github.com/libp2p/specs/tree/master/pubsub).

## Install

```bash
git clone https://github.com/libp2p/rust-libp2p.git;
```

## Usage

TODO

## Contribute

Contributions are welcome! Please check out [the issues](https://github.com/libp2p/go-floodsub/issues).

Small note: If editing the README, please conform to the [standard-readme](https://github.com/RichardLitt/standard-readme) specification.

## License

[MIT](LICENSE)
