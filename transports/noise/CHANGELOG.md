## 0.43.2

- Update x25519-dalek to 2.0.0.

## 0.43.1

- Update dependencies.

## 0.43.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Remove deprecated APIs. See [PR 3511].

- Add `Config::with_webtransport_certhashes`. See [PR 3991].
  This can be used by WebTransport implementers to send (responder) or verify (initiator) certhashes.

[PR 3511]: https://github.com/libp2p/rust-libp2p/pull/3511
[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3991

## 0.42.2

- Deprecate all noise handshakes apart from XX.
  This deprecates `NoiseConfig` and `NoiseAuthenticated` in favor of a new `libp2p_noise::Config` struct.
  In addition, we deprecate all types with a `Noise` prefix.
  Users are encouraged to import the `noise` module and refer to types as `noise::Error` etc.
  See [PR 3768].

[PR 3768]: https://github.com/libp2p/rust-libp2p/pull/3768

## 0.42.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.42.0

- Update to `libp2p-core` `v0.39.0`.

- Deprecate non-compliant noise implementation. We intend to remove it in a future release without replacement. See [PR 3227].

- Deprecate `LegacyConfig` without replacement. See [PR 3265].

[PR 3227]: https://github.com/libp2p/rust-libp2p/pull/3227
[PR 3265]: https://github.com/libp2p/rust-libp2p/pull/3265

## 0.41.0

- Remove `prost::Error` from public API. See [PR 3058].

- Update to `libp2p-core` `v0.38.0`.

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

- Introduce more variants to `NoiseError` to better differentiate between failure cases during authentication. See [PR 2972].

[PR 3058]: https://github.com/libp2p/rust-libp2p/pull/3058
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090
[PR 2972]: https://github.com/libp2p/rust-libp2p/pull/2972

## 0.40.0

- Update to `libp2p-core` `v0.37.0`.

- Introduce `NoiseAuthenticated::xx` constructor, assuming a X25519 DH key exchange. An XX key exchange and X25519 keys
  are the most common way of using noise in libp2p and thus deserve a convenience constructor. See [PR 2887].
- Add `NoiseConfig::with_prologue` which allows users to set the noise prologue of the handshake. See [PR 2903].
- Remove `Deref` implementation on `AuthenticKeypair`. See [PR 2909].
- Make `handshake` module private. See [PR 2909].
- Deprecate `AuthenticKeypair::into_identity`. See [PR 2909].

[PR 2887]: https://github.com/libp2p/rust-libp2p/pull/2887
[PR 2903]: https://github.com/libp2p/rust-libp2p/pull/2903
[PR 2909]: https://github.com/libp2p/rust-libp2p/pull/2909

## 0.39.0

- Update to `libp2p-core` `v0.36.0`.

## 0.38.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.37.0

- Update to `libp2p-core` `v0.34.0`.

## 0.36.0

- Update to `libp2p-core` `v0.33.0`.

## 0.35.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `snow` `v0.9.0`. See [PR 2472].

[PR 2472]: https://github.com/libp2p/rust-libp2p/pull/2472

## 0.34.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.33.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

## 0.32.0 [2021-07-12]

- Update dependencies.

## 0.31.0 [2021-05-17]

- Update to `snow` `v0.8.0` ([PR-2068]).

[PR-2068]: https://github.com/libp2p/rust-libp2p/pull/2068

## 0.30.0 [2021-03-17]

- Update `libp2p-core`.

## 0.29.0 [2021-01-12]

- Update dependencies.

## 0.28.0 [2020-12-17]

- Update `libp2p-core`.

## 0.27.0 [2020-11-25]

- Update `libp2p-core`.

## 0.26.0 [2020-11-09]

- Update dependencies.

## 0.25.0 [2020-10-16]

- Update dependencies.

## 0.24.0 [2020-09-09]

- Bump `libp2p-core` dependency.

- Remove fallback legacy handshake payload decoding by default.
To continue supporting inbound legacy handshake payloads,
`recv_legacy_handshake` must be configured on the `LegacyConfig`.

## 0.23.0 [2020-08-18]

- Bump `libp2p-core` dependency.

## 0.22.0 [2020-08-03]

**NOTE**: For a smooth upgrade path from `0.20` to `> 0.21`
on an existing deployment, this version must not be skipped
or the provided `LegacyConfig` used!

- Stop sending length-prefixed protobuf frames in handshake
payloads by default. See [issue 1631](https://github.com/libp2p/rust-libp2p/issues/1631).
The new `LegacyConfig` is provided to optionally
configure sending the legacy handshake. Note: This release
always supports receiving legacy handshake payloads. A future
release will also move receiving legacy handshake payloads
into a `LegacyConfig` option. However, all legacy configuration
options will eventually be removed, so this is primarily to allow
delaying the handshake upgrade or keeping compatibility with a network
whose peers are slow to upgrade, without having to freeze the
version of `libp2p-noise` altogether in these projects.

## 0.21.0 [2020-07-17]

**NOTE**: For a smooth upgrade path from `0.20` to `> 0.21`
on an existing deployment, this version must not be skipped!

- Add support for reading handshake protobuf frames without
length prefixes in preparation for no longer sending them.
See [issue 1631](https://github.com/libp2p/rust-libp2p/issues/1631).

- Update the `snow` dependency to the latest patch version.

## 0.20.0 [2020-07-01]

- Updated dependencies.
- Conditional compilation fixes for the `wasm32-wasi` target
  ([PR 1633](https://github.com/libp2p/rust-libp2p/pull/1633)).

## 0.19.1 [2020-06-22]

- Re-add noise upgrades for IK and IX
  ([PR 1580](https://github.com/libp2p/rust-libp2p/pull/1580)).

- Updated dependencies.
