
- Add TCP transport to wasm-ext. Update the underlying wasm-ext `ffi` module's API.
  See [PR 4253]

[PR 4253]: https://github.com/libp2p/rust-libp2p/pull/4253

## 0.40.0

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.39.0

- Update to `libp2p-core` `v0.39.0`.

## 0.38.0

- Update to `libp2p-core` `v0.38.0`.

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.37.0

- Update to `libp2p-core` `v0.37.0`.

## 0.36.0

- Update to `libp2p-core` `v0.36.0`.

## 0.35.0

- Update to `libp2p-core` `v0.35.0`.

## 0.34.0

- Update to `libp2p-core` `v0.34.0`.
- Add `Transport::poll` and `Transport::remove_listener` and remove `Transport::Listener`
  for `ExtTransport`. Drive the `Listen` streams within `ExtTransport`. See [PR 2652].

[PR 2652]: https://github.com/libp2p/rust-libp2p/pull/2652

## 0.33.0

- Update to `libp2p-core` `v0.33.0`.

## 0.32.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

## 0.31.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.30.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

## 0.29.0 [2021-07-12]

- Update dependencies.

## 0.28.2 [2021-04-27]

- Support dialing `Multiaddr` with `/p2p` protocol [PR
  2058](https://github.com/libp2p/rust-libp2p/pull/2058).

## 0.28.1 [2021-04-01]

- Require at least js-sys v0.3.50 [PR
  2023](https://github.com/libp2p/rust-libp2p/pull/2023).

## 0.28.0 [2021-03-17]

- Update `libp2p-core`.

## 0.27.0 [2021-01-12]

- Update dependencies.

## 0.26.0 [2020-12-17]

- Update `libp2p-core`.

## 0.25.0 [2020-11-25]

- Update `libp2p-core`.

## 0.24.0 [2020-11-09]

- Fix the WebSocket implementation parsing `x-parity-ws` multiaddresses as `x-parity-wss`.
- Update dependencies.

## 0.23.0 [2020-10-16]

- Update `libp2p-core` dependency.

## 0.22.0 [2020-09-09]

- Update `libp2p-core` dependency.

## 0.21.0 [2020-08-18]

- Update `libp2p-core` dependency.

## 0.20.1 [2020-07-06]

- Improve the code quality of the `websockets.js` binding with the browser's `WebSocket` API.

## 0.20.0 [2020-07-01]

- Updated dependencies.
- Support `/dns` in the websocket implementation
  ([PR 1626](https://github.com/libp2p/rust-libp2p/pull/1626))
