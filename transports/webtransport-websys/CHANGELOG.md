## 0.6.1

- Require `wasm-bindgen-futures` 0.4.80 or later instead of exactly 0.4.58. The exact pin held all
  dependents at `wasm-bindgen` 0.2.108; they now need `wasm-bindgen` 0.2.130 or later, and a
  matching `wasm-bindgen-cli`.
  See [PR 6747](https://github.com/libp2p/rust-libp2p/pull/6747).

## 0.6.0

- Raise MSRV to 1.88.0.
  See [PR 6273](https://github.com/libp2p/rust-libp2p/pull/6273).

## 0.5.2

- Remove `poll_flush called after poll_close` assertion.
  See [PR 6193](https://github.com/libp2p/rust-libp2p/pull/6193).

## 0.5.1

- Remove `once_cell` dependency.
  See [PR 5913](https://github.com/libp2p/rust-libp2p/pull/5913)

## 0.5.0

- Fix `cargo clippy` warnings in `rustc 1.84.0-beta.1`.
  See [PR 5700](https://github.com/libp2p/rust-libp2p/pull/5700).

<!-- Update to libp2p-core v0.43.0 -->

## 0.4.0

- Implement refactored `Transport`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)
- Bump version of web-sys and wasm-bindgen.
  See [PR 5569](https://github.com/libp2p/rust-libp2p/pull/5569) 

## 0.3.0

* Fix unhandled exceptions thrown when calling `Webtransport::close`.
  See [PR 5390](https://github.com/libp2p/rust-libp2p/pull/5390).
* Change logs to debug level.
  See [PR 5396](https://github.com/libp2p/rust-libp2p/pull/5396).


## 0.2.0


## 0.1.0

* Initial implementation of WebTransport transport using web-sys bindings. See [PR 4015].

[PR 4015]: https://github.com/libp2p/rust-libp2p/pull/4015
