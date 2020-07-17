# 0.21.0 [2020-07-17]

**NOTE**: For a smooth upgrade path from `0.20` to `> 0.21`
on an existing deployment, this version must not be skipped!

- Add support for reading handshake protobuf frames without
length prefixes in preparation for no longer sending them.
See [issue 1631](https://github.com/libp2p/rust-libp2p/issues/1631).

- Update the `snow` dependency to the latest patch version.

# 0.20.0 [2020-07-01]

- Updated dependencies.
- Conditional compilation fixes for the `wasm32-wasi` target
  ([PR 1633](https://github.com/libp2p/rust-libp2p/pull/1633)).

# 0.19.1 [2020-06-22]

- Re-add noise upgrades for IK and IX
  ([PR 1580](https://github.com/libp2p/rust-libp2p/pull/1580)).

- Updated dependencies.
