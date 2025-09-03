## 0.43.0

<!-- Update to libp2p-core v0.43.0 -->

## 0.42.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.41.0

- Migrate to `{In,Out}boundConnectionUpgrade` traits.
  See [PR 4695](https://github.com/libp2p/rust-libp2p/pull/4695).
- Remove deprecated type-aliases and make `Config::local_public_key` private.
  See [PR 4734](https://github.com/libp2p/rust-libp2p/pull/4734).

## 0.40.1

- Rename `Plaintext2Config` to `Config` to follow naming conventions across repository.
  See [PR 4535](https://github.com/libp2p/rust-libp2p/pull/4535).

## 0.40.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Remove `Plaintext1Config`.
  Use `Plaintext2Config` instead.
  See [PR 3915].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3915]: https://github.com/libp2p/rust-libp2p/pull/3915

## 0.39.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.39.0

- Update to `libp2p-core` `v0.39.0`.

## 0.38.0

- Add more specific error reporting and remove `prost::Error` from public API. See [PR 3058].

- Update to `libp2p-core` `v0.38.0`.

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 3058]: https://github.com/libp2p/rust-libp2p/pull/3058
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.37.0

- Update to `libp2p-core` `v0.37.0`.

## 0.36.0

- Update to `libp2p-core` `v0.36.0`.

## 0.35.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.34.0

- Update to `libp2p-core` `v0.34.0`.

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

## 0.28.0 [2021-03-17]

- Update `libp2p-core`.

## 0.27.1 [2021-02-15]

- Update dependencies.

## 0.27.0 [2021-01-12]

- Update dependencies.

## 0.26.0 [2020-12-17]

- Update `libp2p-core`.

## 0.25.0 [2020-11-25]

- Update `libp2p-core`.

## 0.24.1 [2020-11-11]

- Ensure that no follow-up protocol data is dropped at the end of the
  plaintext protocol handshake.
  [PR 1831](https://github.com/libp2p/rust-libp2p/pull/1831).

## 0.24.0 [2020-11-09]

- Update dependencies.

## 0.23.0 [2020-10-16]

- Improve error logging
  [PR 1759](https://github.com/libp2p/rust-libp2p/pull/1759).

- Update dependencies.

- Only prefix handshake messages with the message length in bytes as an unsigned
  varint. Return a plain socket once handshaking succeeded. See [issue
  1760](https://github.com/libp2p/rust-libp2p/issues/1760) for details.

## 0.22.0 [2020-09-09]

- Bump `libp2p-core` dependency.

## 0.21.0 [2020-08-18]

- Bump `libp2p-core` dependency.

## 0.20.0 [2020-07-01]

- Updated dependencies.

## 0.19.1 [2020-06-22]

- Updated dependencies.
