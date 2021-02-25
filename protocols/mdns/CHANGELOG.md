# 0.29.0 [unreleased]

- Update `libp2p-swarm`.

# 0.28.1 [2021-02-15]

- Update dependencies.

# 0.28.0 [2021-01-12]

- Update dependencies.

# 0.27.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.26.0 [2020-12-08]

- Create multiple multicast response packets as required to avoid
  hitting the limit of 9000 bytes per MDNS packet.
  [PR 1877](https://github.com/libp2p/rust-libp2p/pull/1877).

- Detect interface changes and join the MDNS multicast
  group on all interfaces as they become available.
  [PR 1830](https://github.com/libp2p/rust-libp2p/pull/1830).

- Replace the use of macros for abstracting over `tokio`
  and `async-std` with the use of `async-io`. As a result
  there may now be an additional reactor thread running
  called `async-io` when using `tokio`, with the futures
  still being polled by the `tokio` runtime.
  [PR 1830](https://github.com/libp2p/rust-libp2p/pull/1830).

# 0.25.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.24.0 [2020-11-09]

- Update dependencies.

# 0.23.0 [2020-10-16]

- Update `libp2p-swarm` and `libp2p-core`.

- Double receive buffer to 4KiB. [PR 1779](https://github.com/libp2p/rust-libp2p/pull/1779/files).

# 0.22.0 [2020-09-09]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.21.0 [2020-08-18]

- Bump `libp2p-core` and `libp2p-swarm` dependencies.

- Allow libp2p-mdns to use either async-std or tokio to drive required UDP
  socket ([PR 1699](https://github.com/libp2p/rust-libp2p/pull/1699)).

# 0.20.0 [2020-07-01]

- Updated dependencies.

# 0.19.2 [2020-06-22]

- Updated dependencies.
