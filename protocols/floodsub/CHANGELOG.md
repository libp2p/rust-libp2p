## 0.45.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.44.0

- Change publish to require `data: impl Into<Bytes>` to internally avoid any costly cloning / allocation.
  See [PR 4754](https://github.com/libp2p/rust-libp2p/pull/4754).

## 0.43.0

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.42.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.42.0

- Update to `libp2p-core` `v0.39.0`.

- Update to `libp2p-swarm` `v0.42.0`.

## 0.41.0

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Replace `Floodsub`'s `NetworkBehaviour` implemention `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.40.0

- Bump rand to 0.8 and quickcheck to 1. See [PR 2857].

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

[PR 2857]: https://github.com/libp2p/rust-libp2p/pull/2857

## 0.39.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

## 0.38.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.37.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

## 0.36.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

## 0.35.0

- Update to `libp2p-swarm` `v0.35.0`.

## 0.34.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

## 0.33.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- Propagate messages only to the target peers and not all connected peers (see [PR2360]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

[PR 2360]: https://github.com/libp2p/rust-libp2p/pull/2360/

## 0.32.0 [2021-11-16]

- Update dependencies.

## 0.31.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

## 0.30.0 [2021-07-12]

- Update dependencies.

- Change `FloodsubDecodeError::ReadError` from a `upgrade::ReadOneError` to
  `std::io::Error`. See [PR 2111].

[PR 2111]: https://github.com/libp2p/rust-libp2p/pull/2111

## 0.29.0 [2021-04-13]

- Update `libp2p-swarm`.

## 0.28.0 [2021-03-17]

- Update `libp2p-swarm`.

- Update dependencies.

## 0.27.0 [2021-01-12]

- Update dependencies.

## 0.26.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.25.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.24.0 [2020-11-09]

- Update dependencies.

## 0.23.0 [2020-10-16]

- Update dependencies.

## 0.22.0 [2020-09-09]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.21.0 [2020-08-18]

- Bump `libp2p-core` and `libp2p-swarm` dependency.

## 0.20.0 [2020-07-01]

- Updated dependencies.

## 0.19.1 [2020-06-22]

- Updated dependencies.
