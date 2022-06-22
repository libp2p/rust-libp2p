# 0.37.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

# 0.36.0

- Update to `libp2p-swarm` `v0.35.0`.

# 0.35.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

# 0.34.0 [2022-01-27]

- Update dependencies.

- Use a random alphanumeric string instead of the local peer ID for mDNS peer
  name (see [PR 2311]).

  Note that previous versions of `libp2p-mdns` expect the peer name to be a
  valid peer ID. Thus they will be unable to discover nodes running this new
  version of `libp2p-mdns`.

- Migrate to Rust edition 2021 (see [PR 2339]).

- Fix generation of peer expiration event and listen on specified IP version (see [PR 2359]).

- Support multiple interfaces (see [PR 2383]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

[PR 2311]: https://github.com/libp2p/rust-libp2p/pull/2311/

[PR 2359]: https://github.com/libp2p/rust-libp2p/pull/2359

[PR 2383]: https://github.com/libp2p/rust-libp2p/pull/2383

# 0.33.0 [2021-11-16]

- Update dependencies.

# 0.32.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Add support for IPv6. To enable set the multicast address
  in `MdnsConfig` to `IPV6_MDNS_MULTICAST_ADDRESS`.
  See [PR 2161] for details.

- Prevent timers from firing at the same time. See [PR 2212] for details.

[PR 2161]: https://github.com/libp2p/rust-libp2p/pull/2161/
[PR 2212]: https://github.com/libp2p/rust-libp2p/pull/2212/

# 0.31.0 [2021-07-12]

- Update dependencies.

# 0.30.2 [2021-05-06]

- Fix discovered event emission.
  [PR 2065](https://github.com/libp2p/rust-libp2p/pull/2065)

# 0.30.1 [2021-04-21]

- Fix timely discovery of peers after listening on a new address.
  [PR 2053](https://github.com/libp2p/rust-libp2p/pull/2053/)

# 0.30.0 [2021-04-13]

- Derive `Debug` and `Clone` for `MdnsConfig`.

- Update `libp2p-swarm`.

# 0.29.0 [2021-03-17]

- Introduce `MdnsConfig` with configurable TTL of discovered peer
  records and configurable multicast query interval. The default
  query interval is increased from 20 seconds to 5 minutes, to
  significantly reduce bandwidth usage. To ensure timely peer
  discovery in the majority of cases, a multicast query is
  initiated whenever a change on a network interface is detected,
  which includes MDNS initialisation at node startup. If necessary
  the MDNS query interval can be reduced via the `MdnsConfig`.
  The `MdnsService` has been removed from the public API, making
  it compulsory that all uses occur through the `Mdns` `NetworkBehaviour`.
  An `MdnsConfig` must now be given to `Mdns::new()`.
  [PR 1977](https://github.com/libp2p/rust-libp2p/pull/1977).

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
