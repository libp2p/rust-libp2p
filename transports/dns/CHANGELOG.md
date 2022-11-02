# 0.37.0

- Remove default features. If you previously depended on `async-std` you need to enable this explicitly now. See [PR 2918].

- Update to `libp2p-core` `v0.37.0`.

[PR 2918]: https://github.com/libp2p/rust-libp2p/pull/2918

# 0.36.0

- Update to `libp2p-core` `v0.36.0`.

# 0.35.0

- Update to `libp2p-core` `v0.35.0`.

# 0.34.0

- Update to `libp2p-core` `v0.34.0`.

# 0.33.0

- Update to `libp2p-core` `v0.33.0`.

- Remove implementation of `Clone` on `GenDnsConfig`. See [PR 2682].

[PR 2682]: https://github.com/libp2p/rust-libp2p/pull/2682

# 0.32.1

- Update to `trust-dns` `v0.21`. See [PR 2543].

[PR 2543]: https://github.com/libp2p/rust-libp2p/pull/2543

# 0.32.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

# 0.31.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

# 0.30.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

# 0.29.0 [2021-07-12]

- Update dependencies.

# 0.28.1 [2021-04-01]

- Update dependencies.

- Remove `fqdn` function optimization conflicting with non fully qualified
  domain name resolution [PR
  2027](https://github.com/libp2p/rust-libp2p/pull/2027).

# 0.28.0 [2021-03-17]

- Update `libp2p-core`.

- Add support for resolving `/dnsaddr` addresses.

- Use `trust-dns-resolver`, removing the internal thread pool and
  expanding the configurability of `libp2p-dns` by largely exposing the
  configuration of `trust-dns-resolver`.
  [PR 1927](https://github.com/libp2p/rust-libp2p/pull/1927)

# 0.27.0 [2021-01-12]

- Update dependencies.

# 0.26.0 [2020-12-17]

- Update `libp2p-core`.

# 0.25.0 [2020-11-25]

- Update `libp2p-core`.

# 0.24.0 [2020-11-09]

- Update dependencies.

# 0.23.0 [2020-10-16]

- Bump `libp2p-core` dependency.

# 0.22.0 [2020-09-09]

- Bump `libp2p-core` dependency.

# 0.21.0 [2020-08-18]

- Bump `libp2p-core` dependency.

# 0.20.0 [2020-07-01]

- Dependency and documentation updates.
