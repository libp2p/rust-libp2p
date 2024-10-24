## 0.44.1

- fix: Return `Error::InvalidMultiaddr` when dialed to a `/dnsaddr` address
  See [PR 5613](https://github.com/libp2p/rust-libp2p/pull/5613)

## 0.44.0

- Implement refactored `Transport`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)
- Allow wss connections on IP addresses.
  See [PR 5525](https://github.com/libp2p/rust-libp2p/pull/5525).
- Add support for `/tls/ws` and keep `/wss` backward compatible.
  See [PR 5523](https://github.com/libp2p/rust-libp2p/pull/5523).

## 0.43.2

- fix: Avoid websocket panic on polling after errors. See [PR 5482].

[PR 5482]: https://github.com/libp2p/rust-libp2p/pull/5482

## 0.43.1

## 0.43.0


## 0.42.1

- Bump `futures-rustls` to `0.24.0`.
  This is a part of the resolution of the [RUSTSEC-2023-0052].
  See [PR 4378].

[PR 4378]: https://github.com/libp2p/rust-libp2p/pull/4378
[RUSTSEC-2023-0052]: https://rustsec.org/advisories/RUSTSEC-2023-0052.html

## 0.42.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Remove `WsConfig::use_deflate` option.
  This allows us to remove the dependency on the `zlib` shared library.
  See [PR 3949].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3949]: https://github.com/libp2p/rust-libp2p/pull/3949

## 0.41.0

- Update to `libp2p-core` `v0.39.0`.

## 0.40.0

- Update to `libp2p-core` `v0.38.0`.

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.39.0

- Update to `libp2p-core` `v0.37.0`.

## 0.38.0

- Update to `libp2p-core` `v0.36.0`.

## 0.37.0

- Update to `libp2p-core` `v0.35.0`.

## 0.36.0

- Update to `libp2p-core` `v0.34.0`.
- Add `Transport::poll` and `Transport::remove_listener` and remove `Transport::Listener`
  for `WsConfig`. See [PR 2652].

[PR 2652]: https://github.com/libp2p/rust-libp2p/pull/2652

## 0.35.0

- Update to `libp2p-core` `v0.33.0`.

- Remove implementation of `Clone` on `WsConfig`. See [PR 2682].

[PR 2682]: https://github.com/libp2p/rust-libp2p/pull/2682

## 0.34.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

## 0.33.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## v0.32.0 [2021-11-16]

- Handle websocket CLOSE with reason code (see [PR 2085]).

[PR 2085]: https://github.com/libp2p/rust-libp2p/pull/2085

## 0.31.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

## 0.30.0 [2021-07-12]

- Update dependencies.

## 0.29.0 [2021-03-17]

- Update `libp2p-core`.

- Permit dialing `/p2p` addresses.

## 0.28.0 [2021-01-12]

- Update dependencies.

## 0.27.0 [2020-12-17]

- Update `libp2p-core`.

## 0.26.3 [2020-12-10]

- Update `async-tls`.

## 0.26.2 [2020-12-09]

- Update minimum patch version for `async-tls`.

## 0.26.1 [2020-12-07]

- Update `rustls`.

## 0.26.0 [2020-11-25]

- Update dependencies.

## 0.25.0 [2020-11-09]

- Update dependencies.

## 0.24.0 [2020-10-16]

- Update dependencies.

## 0.23.0 [2020-09-09]

- Bump `libp2p-core` dependency.

## 0.22.0 [2020-08-18]

- Bump `libp2p-core` dependency.

## 0.21.1 [2020-07-09]

- Update `async-tls` and `rustls` dependency.

## 0.21.0 [2020-07-02]

- Update `libp2p-core`.

## 0.20.0 [2020-06-22]

- Updated `soketto` dependency which caused some smaller
  API changes ([PR 1603](https://github.com/libp2p/rust-libp2p/pull/1603)).
