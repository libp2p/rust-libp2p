# 0.35.0 [unreleased]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- Add metrics for network and configuration performance analysis (see [PR 2346]).

- Improve bandwidth performance by tracking IWANTs and reducing duplicate sends
  (see [PR 2327]).

- Fix `GossipsubConfigBuilder::build()` requiring `&self` to live for `'static` (see [PR 2409])

[PR 2346]: https://github.com/libp2p/rust-libp2p/pull/2346
[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2327]: https://github.com/libp2p/rust-libp2p/pull/2327
[PR 2409]: https://github.com/libp2p/rust-libp2p/pull/2409

# 0.34.0 [2021-11-16]

- Add topic and mesh metrics (see [PR 2316]).

- Fix bug in internal peer's topics tracking (see [PR 2325]).

- Use `instant` and `futures-timer` instead of `wasm-timer` (see [PR 2245]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245
[PR 2325]: https://github.com/libp2p/rust-libp2p/pull/2325
[PR 2316]: https://github.com/libp2p/rust-libp2p/pull/2316

# 0.33.0 [2021-11-01]

- Add an event to register peers that do not support the gossipsub protocol
  [PR 2241](https://github.com/libp2p/rust-libp2p/pull/2241)

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Improve internal peer tracking.
  [PR 2175](https://github.com/libp2p/rust-libp2p/pull/2175)

- Update dependencies.

- Allow `message_id_fn`s to accept closures that capture variables.
  [PR 2103](https://github.com/libp2p/rust-libp2p/pull/2103)

- Implement std::error::Error for error types.
  [PR 2254](https://github.com/libp2p/rust-libp2p/pull/2254)

# 0.32.0 [2021-07-12]

- Update dependencies.

- Reduce log levels across the crate to lessen noisiness of libp2p-gossipsub (see [PR 2101]).

[PR 2101]: https://github.com/libp2p/rust-libp2p/pull/2101

# 0.31.0 [2021-05-17]

- Keep connections to peers in a mesh alive. Allow closing idle connections to peers not in a mesh
  [PR-2043].

[PR-2043]: https://github.com/libp2p/rust-libp2p/pull/2043https://github.com/libp2p/rust-libp2p/pull/2043

# 0.30.1 [2021-04-27]

- Remove `regex-filter` feature flag thus always enabling `regex::RegexSubscriptionFilter` [PR
  2056](https://github.com/libp2p/rust-libp2p/pull/2056).

# 0.30.0 [2021-04-13]

- Update `libp2p-swarm`.

- Update dependencies.

# 0.29.0 [2021-03-17]

- Update `libp2p-swarm`.

- Update dependencies.

# 0.28.0 [2021-02-15]

- Prevent non-published messages being added to caches.
  [PR 1930](https://github.com/libp2p/rust-libp2p/pull/1930)

- Update dependencies.

# 0.27.0 [2021-01-12]

- Update dependencies.

- Implement Gossipsub v1.1 specification.
  [PR 1720](https://github.com/libp2p/rust-libp2p/pull/1720)

# 0.26.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.25.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.24.0 [2020-11-09]

- Update dependencies.

# 0.23.0 [2020-10-16]

- Update dependencies.

# 0.22.0 [2020-09-09]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.21.0 [2020-08-18]

- Add public API to list topics and peers. [PR 1677](https://github.com/libp2p/rust-libp2p/pull/1677).

- Add message signing and extended privacy/validation configurations. [PR 1583](https://github.com/libp2p/rust-libp2p/pull/1583).

- `Debug` instance for `Gossipsub`. [PR 1673](https://github.com/libp2p/rust-libp2p/pull/1673).

- Bump `libp2p-core` and `libp2p-swarm` dependency.

# 0.20.0 [2020-07-01]

- Updated dependencies.

# 0.19.3 [2020-06-23]

- Maintenance release fixing linter warnings.

# 0.19.2 [2020-06-22]

- Updated dependencies.
