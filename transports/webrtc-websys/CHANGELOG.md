## 0.5.1

- Require `wasm-bindgen-futures` 0.4.80 or later instead of exactly 0.4.58. The exact pin held all
  dependents at `wasm-bindgen` 0.2.108; they now need `wasm-bindgen` 0.2.130 or later, and a
  matching `wasm-bindgen-cli`.
  See [PR 6671](https://github.com/libp2p/rust-libp2p/pull/6671).

- Fix compilation with `panic = "unwind"` (e.g. `wasm32-unknown-emscripten`) and `wasm-bindgen`
  0.2.109 or later.
  See [PR 6671](https://github.com/libp2p/rust-libp2p/pull/6671).

## 0.5.0

- Require `getrandom/js` feature only under `wasm` target.
  See [PR 6102](https://github.com/libp2p/rust-libp2p/pull/6102)

- Raise MSRV to 1.88.0.
  See [PR 6273](https://github.com/libp2p/rust-libp2p/pull/6273).

## 0.4.0

- Cut stable release.
  See [PR 5807](https://github.com/libp2p/rust-libp2p/pull/5807)
- Bump version of web-sys and update `__Nonexhaustive` to `__Invalid`.
  See [PR 5569](https://github.com/libp2p/rust-libp2p/pull/5569)

<!-- Update to libp2p-core v0.43.0 -->

## 0.4.0-alpha

- Implement refactored `Transport`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)

## 0.3.0-alpha

- Bump version in order to publish a new version dependent on latest `libp2p-core`.
  See [PR 4959](https://github.com/libp2p/rust-libp2p/pull/4959).
- Remove `libp2p_noise` from the public API.
  See [PR 4969](https://github.com/libp2p/rust-libp2p/pull/4969).

## 0.2.0-alpha

- Rename `Error::JsError` to `Error::Js`.
  See [PR 4653](https://github.com/libp2p/rust-libp2p/pull/4653)

## 0.1.0-alpha

- Initial alpha release.
  See [PR 4248].

[PR 4248]: https://github.com/libp2p/rust-libp2p/pull/4248
