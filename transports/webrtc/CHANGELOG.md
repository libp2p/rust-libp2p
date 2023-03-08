[unreleased]

-   fix for [#3574](https://github.com/libp2p/rust-libp2p/issues/3574) by not emitting `UDPMuxEvent::Error` when `ErrorKind::ConnectionReset` occurs from remote client

# 0.4.0-alpha.2

- Update to `libp2p-noise` `v0.42.0`.

- Update to `libp2p-core` `v0.39.0`.

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

# 0.4.0-alpha

- Initial alpha release.