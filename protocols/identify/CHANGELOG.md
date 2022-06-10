# 0.36.1

- Allow at most one inbound identify push stream.

# 0.36.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- Expose explicits errors via `UpgradeError` instead of generic `io::Error`. See [PR 2630].

[PR 2630]: https://github.com/libp2p/rust-libp2p/pull/2630
# 0.35.0

- Update to `libp2p-swarm` `v0.35.0`.

# 0.34.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

# 0.33.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

# 0.32.0 [2021-11-16]

- Use `futures-timer` instead of `wasm-timer` (see [PR 2245]).
- Filter invalid peers from cache used in `addresses_of_peer` – [PR 2338].

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245
[PR 2338]: https://github.com/libp2p/rust-libp2p/pull/2338

# 0.31.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Assist in peer discovery by optionally returning reported listen addresses
  of other peers from `addresses_of_peer` (see [PR
  2232](https://github.com/libp2p/rust-libp2p/pull/2232)), disabled by default.

# 0.30.0 [2021-07-12]

- Update dependencies.

# 0.29.0 [2021-04-13]

- Add support for configurable automatic push to connected peers
  on listen addr changes. Disabled by default.
  [PR 2004](https://github.com/libp2p/rust-libp2p/pull/2004)

- Implement the `/ipfs/id/push/1.0.0` protocol.
  cf. https://github.com/libp2p/specs/tree/master/identify#identifypush
  [PR 1999](https://github.com/libp2p/rust-libp2p/pull/1999)

- Emit `IdentifyEvent::Pushed` event after successfully pushing identification
  information to peer [PR
  2030](https://github.com/libp2p/rust-libp2p/pull/2030).

# 0.28.0 [2021-03-17]

- Update `libp2p-swarm`.

- Update dependencies.

# 0.27.0 [2021-01-12]

- Update dependencies.

# 0.26.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.25.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.24.0 [2020-11-09]

- Update dependencies.

# 0.23.0 [2020-10-16]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.22.0 [2020-09-09]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.21.0 [2020-08-18]

- Bump `libp2p-core` and `libp2p-swarm` dependencies.

# 0.20.0 [2020-07-01]

- Updated dependencies.

# 0.19.2 [2020-06-22]

- Updated dependencies.
