## 0.42.0

- Implement refactored `Transport`.
  See [PR 4568]
- Deprecate `port_reuse` setting, as this is now decided by the behaviour, not the transport.
  See [PR 4568]

[PR 4568]: https://github.com/libp2p/rust-libp2p/pull/4568

## 0.41.1

- Disable Nagle's algorithm (i.e. `TCP_NODELAY`) by default.
  See [PR 4916](https://github.com/libp2p/rust-libp2p/pull/4916)

## 0.41.0


## 0.40.1

- Expose `async_io::TcpStream`.
  See [PR 4683](https://github.com/libp2p/rust-libp2p/pull/4683).

## 0.40.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Remove deprecated items. See [PR 3978].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3978]: https://github.com/libp2p/rust-libp2p/pull/3978

## 0.39.0

- Update to `libp2p-core` `v0.39.0`.
- Fix a bug where we removed any other listener in `Transport::remove_listener` except for the one with the provided `ListenerId`. See [PR 3387].

[PR 3387]: https://github.com/libp2p/rust-libp2p/pull/3387

## 0.38.0

- Update to `if-watch`  `v3.0.0` and pass through `tokio` and `async-io` features. See [PR 3101].

- Deprecate types with `Tcp` prefix (`GenTcpConfig`, `TcpTransport` and `TokioTcpTransport`) in favor of referencing them by module / crate. See [PR 2961].

- Remove `TcpListenStream` and `TcpListenerEvent` from public API. See [PR 2961].

- Update to `libp2p-core` `v0.38.0`.

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 3101]: https://github.com/libp2p/rust-libp2p/pull/3101
[PR 2961]: https://github.com/libp2p/rust-libp2p/pull/2961
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.37.0

- Update to `if-watch` `v2.0.0`. Simplify `IfWatcher` integration.
  Use `if_watch::IfWatcher` for all runtimes. See [PR 2813].

- Update to `libp2p-core` `v0.37.0`.

- Remove default features. If you previously depended on `async-std` you need to enable this explicitly now. See [PR 2918].

- Return `None` in `GenTcpTransport::address_translation` if listen- or observed address are not tcp addresses.
  See [PR 2970].

[PR 2813]: https://github.com/libp2p/rust-libp2p/pull/2813
[PR 2918]: https://github.com/libp2p/rust-libp2p/pull/2918
[PR 2970]: https://github.com/libp2p/rust-libp2p/pull/2970

## 0.36.0

- Update to `libp2p-core` `v0.36.0`.

## 0.35.0

- Update to `libp2p-core` `v0.35.0`.

- Update to `if-watch` `v1.1.1`.

## 0.34.0

- Update to `libp2p-core` `v0.34.0`.

- Call `TcpStream::take_error` in tokio `Provider` to report connection
  establishment errors early. See also [PR 2458] for the related async-io
  change.

- Split `GenTcpConfig` into `GenTcpConfig` and `GenTcpTransport`. Drive the `TcpListenStream`s
  within the `GenTcpTransport`. Add `Transport::poll` and `Transport::remove_listener`
  for `GenTcpTransport`. See [PR 2652].

[PR 2652]: https://github.com/libp2p/rust-libp2p/pull/2652

## 0.33.0

- Update to `libp2p-core` `v0.33.0`.

- Remove implementation of `Clone` on `GenTcpConfig`. See [PR 2682].

[PR 2682]: https://github.com/libp2p/rust-libp2p/pull/2682

## 0.32.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

## 0.31.1 [2022-02-02]

- Call `TcpSocket::take_error` to report connection establishment errors early. See [PR 2458].

[PR 2458]: https://github.com/libp2p/rust-libp2p/pull/2458

## 0.31.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- When using PortReuse::Enabled, bind to INADDR_ANY to avoid picking the wrong IP (see [PR 2382]).

[PR 2382]: https://github.com/libp2p/rust-libp2p/pull/2382
[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.30.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

## 0.29.0 [2021-07-12]

- Update dependencies.

## 0.28.0 [2021-03-17]

- Update `libp2p-core`.

- Permit `/p2p` addresses.

- Update to `if-watch-0.2`.

## 0.27.1 [2021-02-15]

- Update dependencies.

## 0.27.0 [2021-01-12]

- Add support for port reuse and (re)add transport-specific
  address translation. Thereby use only `async-io` instead of
  `async-std`, renaming the feature accordingly. `async-io`
  is a default feature, with an additional `tokio` feature
  as before.
  [PR 1887](https://github.com/libp2p/rust-libp2p/pull/1887)

- Update dependencies.

## 0.26.0 [2020-12-17]

- Update `async-io`.

## 0.25.1 [2020-11-26]

- Lower `async-std` version to `1.6`, for compatibility
  with other libp2p crates.

## 0.25.0 [2020-11-25]

- Update `libp2p-core`.

## 0.24.0 [2020-11-09]

- Update dependencies.

## 0.23.0 [2020-10-16]

- Update `libp2p-core`.

- Replace `get_if_addrs` with `if-addrs`.

## 0.22.0 [2020-09-09]

- Bump `libp2p-core` dependency.

## 0.21.0 [2020-08-18]

- Bump `libp2p-core` dependency.

## 0.20.0 [2020-07-01]

- Updated dependencies.

## 0.19.2 [2020-06-22]

- Updated dependencies.
