## 0.4.1

- fix: Return `None` when extracting a `/dnsaddr` address
  See [PR 5613](https://github.com/libp2p/rust-libp2p/pull/5613)

- Fix `cargo clippy` warnings in `rustc 1.84.0-beta.1`.
  See [PR 5700](https://github.com/libp2p/rust-libp2p/pull/5700).

## 0.4.0

- Implement refactored `Transport`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)
- Add support for `/tls/ws` and keep `/wss` backward compatible.
  See [PR 5523](https://github.com/libp2p/rust-libp2p/pull/5523).

## 0.3.3

- Fix use-after-free handler invocation from JS side.
  See [PR 5521](https://github.com/libp2p/rust-libp2p/pull/5521).

## 0.3.2

- Change close code in drop implementation to `1000` given that in browsers only
  the code `1000` and codes between `3000` and `4999` are allowed to be set by
  userland code.
  See [PR 5229](https://github.com/libp2p/rust-libp2p/pull/5229).

## 0.3.1

- Add support for different WASM environments by introducing a `WebContext` that
  detects and abstracts the `Window` vs the `WorkerGlobalScope` API.
  See [PR 4889](https://github.com/libp2p/rust-libp2p/pull/4889).

## 0.3.0


## 0.2.0

- Add Websys Websocket transport.

## 0.1.0

- Crate claimed.
