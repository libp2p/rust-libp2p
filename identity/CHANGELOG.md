## 0.1.3

- Fix [RUSTSEC-2022-0093] by updating `ed25519-dalek` to `2.0`.
  See [PR 4337]

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
