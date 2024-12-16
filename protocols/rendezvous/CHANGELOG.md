## 0.15.1

- Update to `libp2p-request-response` `v0.28.0`.

## 0.15.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.14.1
- Use `web-time` instead of `instant`.
  See [PR 5347](https://github.com/libp2p/rust-libp2p/pull/5347).

## 0.14.0


## 0.13.1
- Refresh registration upon a change in external addresses.
  See [PR 4629].

[PR 4629]: https://github.com/libp2p/rust-libp2p/pull/4629

## 0.13.0

- Changed the signature of the function `client::Behavior::register()`,
  it returns `Result<(), RegisterError>` now.
  Remove the `Remote` variant from `RegisterError` and instead put the information from `Remote`
  directly into the variant from the `Event` enum.
  See [PR 4073].

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 4073]: https://github.com/libp2p/rust-libp2p/pull/4073
[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.12.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.12.0

- Update to `libp2p-core` `v0.39.0`.

- Update to `libp2p-swarm` `v0.42.0`.

## 0.11.0

- De- and encode protobuf messages using `prost-codec`. See [PR 3058].

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Replace `Client` and `Server`'s `NetworkBehaviour` implementation `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3058]: https://github.com/libp2p/rust-libp2p/pull/3058
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.10.0

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

## 0.9.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

## 0.8.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.7.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

## 0.6.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- Renamed `Error::ConversionError` to `Error::Conversion` in the `codec` module. See [PR 2620].

[PR 2620]: https://github.com/libp2p/rust-libp2p/pull/2620

## 0.5.0

- Update to `libp2p-swarm` `v0.35.0`.

## 0.4.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

## 0.3.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.2.0 [2021-11-16]

- Use `instant` and `futures-timer` instead of `wasm-timer` (see [PR 2245]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245

## 0.1.0 [2021-11-01]

- Initial release.
