## 0.2.10

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

## 0.2.9

- Add `rand` feature gate to ecdsa methods requiring a random number generator.
  See [PR 5212](https://github.com/libp2p/rust-libp2p/pull/5212).

## 0.2.8

- Bump `ring` to `0.17.5`.
  See [PR 4779](https://github.com/libp2p/rust-libp2p/pull/4779).

## 0.2.7

- Add `rand` feature to gate methods requiring a random number generator, enabling use in restricted environments (e.g. smartcontracts).
  This feature is not enabled by default.
  See [PR 4349](https://github.com/libp2p/rust-libp2p/pull/4349).

## 0.2.6

- Make `PeerId::to_bytes` and `PeerId::to_base58` take `self` by value to follow Rust convention of `Copy` types.
  See [PR 4653](https://github.com/libp2p/rust-libp2p/pull/4653).

## 0.2.5

- Fix usage of HKDF within `Keypair::derive_secret`.
  See [PR 4554](https://github.com/libp2p/rust-libp2p/pull/4554).

## 0.2.4

- Implement `Keypair::derive_secret`, to deterministically derive a new secret from the embedded secret key.
  See [PR 4554].

[PR 4554]: https://github.com/libp2p/rust-libp2p/pull/4554

## 0.2.3

- Fix [RUSTSEC-2022-0093] by updating `ed25519-dalek` to `2.0`.
  See [PR 4337]

[RUSTSEC-2022-0093]: https://rustsec.org/advisories/RUSTSEC-2022-0093
[PR 4337]: https://github.com/libp2p/rust-libp2p/pull/4337

## 0.2.2

- Implement `from_protobuf_encoding` for RSA `Keypair`.
  See [PR 4193].

[PR 4193]: https://github.com/libp2p/rust-libp2p/pull/4193

## 0.2.1

- Expose `KeyType` for `PublicKey` and `Keypair`.
  See [PR 4107].

[PR 4107]: https://github.com/libp2p/rust-libp2p/pull/4107

## 0.2.0

- Raise MSRV to 1.65.
  See [PR 3715].
- Add support for exporting and importing ECDSA keys via the libp2p [protobuf format].
  See [PR 3863].

- Make `Keypair` and `PublicKey` opaque.
  See [PR 3866].

- Remove `identity::secp256k1::SecretKey::sign_hash` and make `identity::secp256k1::SecretKey::sign` infallible.
  See [PR 3850].

- Remove deprecated items. See [PR 3928].

- Remove `PeerId::try_from_multiaddr`.
  `multiaddr::Protocol::P2p` is now type-safe and contains a `PeerId` directly, rendering this function obsolete.
  See [PR 3656].

- Remove `PeerId::is_public_key` because it is unused and can be implemented externally.
  See [PR 3656].

[PR 3656]: https://github.com/libp2p/rust-libp2p/pull/3656
[PR 3850]: https://github.com/libp2p/rust-libp2p/pull/3850
[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3863]: https://github.com/libp2p/rust-libp2p/pull/3863
[PR 3866]: https://github.com/libp2p/rust-libp2p/pull/3866
[PR 3928]: https://github.com/libp2p/rust-libp2p/pull/3928
[protobuf format]: https://github.com/libp2p/specs/blob/master/peer-ids/peer-ids.md#keys

## 0.1.2

- Add `impl From<ed25519::PublicKey> for PublicKey` so that `PublicKey::from(ed25519::PublicKey)` works.
  See [PR 3805].

[PR 3805]: https://github.com/libp2p/rust-libp2p/pull/3805

- Follow Rust naming conventions for conversion methods.
  See [PR 3775].

[PR 3775]: https://github.com/libp2p/rust-libp2p/pull/3775

## 0.1.1

- Add `From` impl for specific keypairs.
  See [PR 3626].

[PR 3626]: https://github.com/libp2p/rust-libp2p/pull/3626
