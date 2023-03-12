# 0.4.0-alpha.3

- Gracefully handle `ConnectionReset` error on individual connections, avoiding shutdown of the entire listener upon disconnect of a single client.
  See [PR 3575].

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3575]: https://github.com/libp2p/rust-libp2p/pull/3575
[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

# 0.4.0-alpha.2

- Update to `libp2p-noise` `v0.42.0`.

- Update to `libp2p-core` `v0.39.0`.

# 0.4.0-alpha

- Initial alpha release.
