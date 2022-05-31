# 0.36.0

- Update to `libp2p-core` `v0.33.0`.

# 0.35.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `snow` `v0.9.0`. See [PR 2472].

[PR 2472]: https://github.com/libp2p/rust-libp2p/pull/2472

# 0.34.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

# 0.33.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

# 0.32.0 [2021-07-12]

- Update dependencies.

# 0.31.0 [2021-05-17]

- Update to `snow` `v0.8.0` ([PR-2068]).

[PR-2068]: https://github.com/libp2p/rust-libp2p/pull/2068

# 0.30.0 [2021-03-17]

- Update `libp2p-core`.

# 0.29.0 [2021-01-12]

- Update dependencies.

# 0.28.0 [2020-12-17]

- Update `libp2p-core`.

# 0.27.0 [2020-11-25]

- Update `libp2p-core`.

# 0.26.0 [2020-11-09]

- Update dependencies.

# 0.25.0 [2020-10-16]

- Update dependencies.

# 0.24.0 [2020-09-09]

- Bump `libp2p-core` dependency.

- Remove fallback legacy handshake payload decoding by default.
To continue supporting inbound legacy handshake payloads,
`recv_legacy_handshake` must be configured on the `LegacyConfig`.

# 0.23.0 [2020-08-18]

- Bump `libp2p-core` dependency.

# 0.22.0 [2020-08-03]

**NOTE**: For a smooth upgrade path from `0.20` to `> 0.21`
on an existing deployment, this version must not be skipped
or the provided `LegacyConfig` used!

- Stop sending length-prefixed protobuf frames in handshake
payloads by default. See [issue 1631](https://github.com/libp2p/rust-libp2p/issues/1631).
The new `LegacyConfig` is provided to optionally
configure sending the legacy handshake. Note: This release
always supports receiving legacy handshake payloads. A future
release will also move receiving legacy handshake payloads
into a `LegacyConfig` option. However, all legacy configuration
options will eventually be removed, so this is primarily to allow
delaying the handshake upgrade or keeping compatibility with a network
whose peers are slow to upgrade, without having to freeze the
version of `libp2p-noise` altogether in these projects.

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
