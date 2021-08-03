# 0.33.0 [unreleased]

- Update dependencies.

- Allow `message_id_fn`s to accept closures that capture variables.
  [PR 2103](https://github.com/libp2p/rust-libp2p/pull/2103)

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
