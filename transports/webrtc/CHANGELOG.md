## 0.9.0-alpha

<!-- Update to libp2p-core v0.43.0 -->

## 0.8.0-alpha

- Implement refactored `Transport`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)

## 0.7.1-alpha

- Bump `libp2p-webrtc-utils` dependency to `0.2.0`.
  See [PR 5118](https://github.com/libp2p/rust-libp2p/pull/5118).

## 0.7.0-alpha

- Bump version in order to publish a new version dependent on latest `libp2p-core`.
  See [PR 4959](https://github.com/libp2p/rust-libp2p/pull/4959).

## 0.6.1-alpha

- Move common dependencies to `libp2p-webrtc-utils` crate.
  See [PR 4248].

[PR 4248]: https://github.com/libp2p/rust-libp2p/pull/4248

## 0.6.0-alpha

- Update `webrtc` dependency to `v0.8.0`.
  See [PR 4099].

[PR 4099]: https://github.com/libp2p/rust-libp2p/pull/4099

## 0.5.0-alpha

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.4.0-alpha.4

- Make `Fingerprint` type public. See [PR 3648].

[PR 3648]: https://github.com/libp2p/rust-libp2p/pull/3648

## 0.4.0-alpha.3

- Gracefully handle `ConnectionReset` error on individual connections, avoiding shutdown of the entire listener upon disconnect of a single client.
  See [PR 3575].

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3575]: https://github.com/libp2p/rust-libp2p/pull/3575
[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.4.0-alpha.2

- Update to `libp2p-noise` `v0.42.0`.

- Update to `libp2p-core` `v0.39.0`.

## 0.4.0-alpha

- Initial alpha release.
