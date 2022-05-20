# 0.36.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

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

- Use `instant` and `futures-timer` instead of `wasm-timer` (see [PR 2245]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245

# 0.31.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Don't close connection if ping protocol is unsupported by remote.
  Previously, a failed protocol negotation for ping caused a force close of the connection.
  As a result, all nodes in a network had to support ping.
  To allow networks where some nodes don't support ping, we now emit
  `PingFailure::Unsupported` once for every connection on which ping is not supported.

  In case you want to stick with the old behavior, you need to close the connection
  manually on `PingFailure::Unsupported`.

  Fixes [#2109](https://github.com/libp2p/rust-libp2p/issues/2109). Also see [PR 2149].

  [PR 2149]: https://github.com/libp2p/rust-libp2p/pull/2149/

- Rename types as per [discussion 2174].
  `Ping` has been renamed to `Behaviour`.
  The `Ping` prefix has been removed from various types like `PingEvent`.
  Users should prefer importing the ping protocol as a module (`use libp2p::ping;`),
  and refer to its types via `ping::`. For example: `ping::Behaviour` or `ping::Event`.

  [discussion 2174]: https://github.com/libp2p/rust-libp2p/discussions/2174

# 0.30.0 [2021-07-12]

- Update dependencies.

# 0.29.0 [2021-04-13]

- Update `libp2p-swarm`.

# 0.28.0 [2021-03-17]

- Update `libp2p-swarm`.

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

- Ensure the outbound ping is flushed before awaiting
  the response. Otherwise the behaviour depends on
  implementation details of the stream muxer used.
  The current behaviour resulted in stalls with Mplex.

# 0.22.0 [2020-09-09]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.21.0 [2020-08-18]

- Refactor the ping protocol for conformity by (re)using
a single substream for outbound pings, addressing
[#1601](https://github.com/libp2p/rust-libp2p/issues/1601).

- Bump `libp2p-core` and `libp2p-swarm` dependencies.

# 0.20.0 [2020-07-01]

- Updated dependencies.

# 0.19.3 [2020-06-22]

- Updated dependencies.

# 0.19.2 [2020-06-18]

- Close substream in inbound upgrade
  [PR 1606](https://github.com/libp2p/rust-libp2p/pull/1606).
