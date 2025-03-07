## 0.42.0

<!-- Update to libp2p-core v0.43.0 -->

## 0.41.0

- Implement refactored `Transport`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)

## 0.40.0


## 0.39.0

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.38.0

- Update to `libp2p-core` `v0.39.0`.

## 0.37.0

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.36.0

- Remove default features. If you previously depended on `async-std` you need to enable this explicitly now. See [PR 2918].

- Update to `libp2p-core` `v0.37.0`.

[PR 2918]: https://github.com/libp2p/rust-libp2p/pull/2918

## 0.35.0

- Update to `libp2p-core` `v0.36.0`.

## 0.34.0

- Update to `libp2p-core` `v0.35.0`.

## 0.33.0

- Update dependencies.
- Update to `libp2p-core` `v0.34.0`.
- Add `Transport::poll` and `Transport::remove_listener` and remove `Transport::Listener` for
  `UdsConfig` Drive listener streams in `UdsConfig` directly. See [PR 2652].

[PR 2652]: https://github.com/libp2p/rust-libp2p/pull/2652

## 0.32.0 [2022-01-27]

- Update to `libp2p-core` `v0.32.0`.

## 0.31.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.30.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

## 0.29.0 [2021-07-12]

- Update dependencies.

## 0.28.0 [2021-03-17]

- Update `libp2p-core`.

- Permit `/p2p` addresses.

## 0.27.0 [2021-01-12]

- Update dependencies.

## 0.26.0 [2020-12-17]

- Update `libp2p-core`.

## 0.25.0 [2020-11-25]

- Update `libp2p-core`.

## 0.24.0 [2020-11-09]

- Update dependencies.

## 0.23.0 [2020-10-16]

- Update `libp2p-core` dependency.

## 0.22.0 [2020-09-09]

- Update `libp2p-core` dependency.

## 0.21.0 [2020-08-18]

- Update `libp2p-core` dependency.

## 0.20.0 [2020-07-01]

- Updated dependencies.
- Conditional compilation fixes for the `wasm32-wasi` target
  ([PR 1633](https://github.com/libp2p/rust-libp2p/pull/1633)).

## 0.19.2 [2020-06-22]

- Updated dependencies.
