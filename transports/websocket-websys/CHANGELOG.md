## 0.3.2

- Add support for environments which globally expose `setInterval` and `clearInterval` instead of a window.
- This crate will now correctly panic if it cannot find the `WebSocket` global JavaScript object.

## 0.3.1

- Add support for different WASM environments by introducing a `WebContext` that
  detects and abstracts the `Window` vs the `WorkerGlobalScope` API.
  See [PR 4889](https://github.com/libp2p/rust-libp2p/pull/4889).

## 0.3.0


## 0.2.0

- Add Websys Websocket transport.

## 0.1.0

- Crate claimed.
