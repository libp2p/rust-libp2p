## 0.1.0

- Initial release: a `Behaviour` and `Control` for sending and receiving
  unreliable datagrams over libp2p connections (QUIC only), implementing the
  `/dg/1` control stream and framing from [libp2p/specs#680]. `Behaviour::new`
  takes the application protocol the datagrams belong to.
  See [PR 6489](https://github.com/libp2p/rust-libp2p/pull/6489).

[libp2p/specs#680]: https://github.com/libp2p/specs/pull/680
