# 0.33.0

- Update to `libp2p-core` `v0.33.0`.

- Remove implementation of `Clone` on `GenTcpConfig`. See [PR 2682].

[PR 2682]: https://github.com/libp2p/rust-libp2p/pull/2682

# 0.32.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

# 0.31.1 [2022-02-02]

- Call `TcpSocket::take_error` to report connection establishment errors early.

# 0.31.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- When using PortReuse::Enabled, bind to INADDR_ANY to avoid picking the wrong IP (see [PR 2382]).

[PR 2382]: https://github.com/libp2p/rust-libp2p/pull/2382
[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

# 0.30.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

# 0.29.0 [2021-07-12]

- Update dependencies.

# 0.28.0 [2021-03-17]

- Update `libp2p-core`.

- Permit `/p2p` addresses.

- Update to `if-watch-0.2`.

# 0.27.1 [2021-02-15]

- Update dependencies.

# 0.27.0 [2021-01-12]

- Add support for port reuse and (re)add transport-specific
  address translation. Thereby use only `async-io` instead of
  `async-std`, renaming the feature accordingly. `async-io`
  is a default feature, with an additional `tokio` feature
  as before.
  [PR 1887](https://github.com/libp2p/rust-libp2p/pull/1887)

- Update dependencies.

# 0.26.0 [2020-12-17]

- Update `async-io`.

# 0.25.1 [2020-11-26]

- Lower `async-std` version to `1.6`, for compatibility
  with other libp2p crates.

# 0.25.0 [2020-11-25]

- Update `libp2p-core`.

# 0.24.0 [2020-11-09]

- Update dependencies.

# 0.23.0 [2020-10-16]

- Update `libp2p-core`.

- Replace `get_if_addrs` with `if-addrs`.

# 0.22.0 [2020-09-09]

- Bump `libp2p-core` dependency.

# 0.21.0 [2020-08-18]

- Bump `libp2p-core` dependency.

# 0.20.0 [2020-07-01]

- Updated dependencies.

# 0.19.2 [2020-06-22]

- Updated dependencies.
