# 0.18.1

- Add `with_p2p` on `Multiaddr`. See [PR 102].

[PR 102]: https://github.com/multiformats/rust-multiaddr/pull/102

# 0.18.0

- Add `WebTransport` instance for `Multiaddr`. See [PR 70].

- Disable all features of `multihash`. See [PR 77].

- Mark `Protocol` as `#[non_exhaustive]`. See [PR 82].

- Rename `Protocol::WebRTC` to `Protocol::WebRTCDirect`.
  See [multiformats/multiaddr discussion] for context.
  Remove deprecated support for `/webrtc` in favor of the existing `/webrtc-direct` string representation.
  **Note that this is a breaking change.**

- Make `/p2p` typesafe, i.e. have `Protocol::P2p` contain a `PeerId` instead of a `Multihash`.
  See [PR 83].

[multiformats/multiaddr discussion]: https://github.com/multiformats/multiaddr/pull/150#issuecomment-1468791586
[PR 70]: https://github.com/multiformats/rust-multiaddr/pull/70
[PR 77]: https://github.com/multiformats/rust-multiaddr/pull/77
[PR 82]: https://github.com/multiformats/rust-multiaddr/pull/82
[PR 83]: https://github.com/multiformats/rust-multiaddr/pull/83

# 0.17.1

- Rename string representation of `WebRTC` protocol from `/webrtc` to `/webrt-direct`.
  For backwards compatibility `/webrtc` will still be decoded to `Protocol::WebRTC`, but `Protocol::WebRTC` will from now on always be encoded as `/webrtc-direct`.
  See [multiformats/multiaddr discussion] and [PR 84] for context.
  ``` rust
  assert_eq!(
      Multiaddr::empty().with(Protocol::WebRTC),
      "/webrtc".parse().unwrap(),
  );
  assert_eq!(
      Multiaddr::empty().with(Protocol::WebRTC),
      "/webrtc-direct".parse().unwrap(),
  );
  assert_eq!(
      "/webrtc-direct",
      Multiaddr::empty().with(Protocol::WebRTC).to_string(),
  );
  assert_ne!(
      "/webrtc",
      Multiaddr::empty().with(Protocol::WebRTC).to_string(),
  );
  ```

[PR 84]: https://github.com/multiformats/rust-multiaddr/pull/84

# 0.17.0

- Update to multihash `v0.17`. See [PR 63].

[PR 63]: https://github.com/multiformats/rust-multiaddr/pull/63

# 0.16.0 [2022-11-04]

- Create `protocol_stack` for Multiaddr. See [PR 60].

- Add `QuicV1` instance for `Multiaddr`. See [PR 64].

[PR 60]: https://github.com/multiformats/rust-multiaddr/pull/60
[PR 64]: https://github.com/multiformats/rust-multiaddr/pull/64

# 0.15.0 [2022-10-20]

- Add `WebRTC` instance for `Multiaddr`. See [PR 59].
- Add `Certhash` instance for `Multiaddr`. See [PR 59].

- Add support for Noise protocol. See [PR 53].

- Use `multibase` instead of `bs58` for base58 encoding. See [PR 56].

[PR 53]: https://github.com/multiformats/rust-multiaddr/pull/53
[PR 56]: https://github.com/multiformats/rust-multiaddr/pull/56
[PR 59]: https://github.com/multiformats/rust-multiaddr/pull/59

# 0.14.0 [2022-02-02]

- Add support for TLS protocol (see [PR 48]).

- Update to `multihash` `v0.15` (see [PR 50]).

- Update to `multihash` `v0.16` (see [PR 51]).

[PR 48]: https://github.com/multiformats/rust-multiaddr/pull/48
[PR 50]: https://github.com/multiformats/rust-multiaddr/pull/50
[PR 50]: https://github.com/multiformats/rust-multiaddr/pull/51

# 0.13.0 [2021-07-08]

- Update to multihash v0.14.0 (see [PR 44]).

- Update to rand v0.8.4 (see [PR 45]).

[PR 44]: https://github.com/multiformats/rust-multiaddr/pull/44
[PR 45]: https://github.com/multiformats/rust-multiaddr/pull/45

# 0.12.0 [2021-05-26]

- Merge  [multiaddr] and [parity-multiaddr] (see [PR 40]).

    - Functionality to go from a `u64` to a `multiadddr::Protocol` and back is
      removed. Please open an issue on [multiaddr] in case this is still needed.

    - Given that `multiaddr::Protocol` now represents both the protocol
      identifier as well as the protocol data (e.g. protocol identifier `55`
      (`dns6`) and protocol data `some-domain.example`) `multiaddr::Protocol` is
      no longer `Copy`.

[multiaddr]: https://github.com/multiformats/rust-multiaddr
[parity-multiaddr]: https://github.com/libp2p/rust-libp2p/blob/master/misc/multiaddr/
[PR 40]: https://github.com/multiformats/rust-multiaddr/pull/40

# 0.11.2 [2021-03-17]

- Add `Multiaddr::ends_with()`.

# 0.11.1 [2021-02-15]

- Update dependencies

# 0.11.0 [2021-01-12]

- Update dependencies

# 0.10.1 [2021-01-12]

- Fix compilation with serde-1.0.119.
  [PR 1912](https://github.com/libp2p/rust-libp2p/pull/1912)

# 0.10.0 [2020-11-25]

- Upgrade multihash to `0.13`.

# 0.9.6 [2020-11-17]

- Move the `from_url` module and functionality behind the `url` feature,
  enabled by default.
  [PR 1843](https://github.com/libp2p/rust-libp2p/pull/1843).

# 0.9.5 [2020-11-14]

- Limit initial memory allocation in `visit_seq`.
  [PR 1833](https://github.com/libp2p/rust-libp2p/pull/1833).

# 0.9.4 [2020-11-09]

- Update dependencies.

# 0.9.3 [2020-10-16]

- Update dependencies.

# 0.9.2 [2020-08-31]

- Add `Ord` instance for `Multiaddr`.

# 0.9.1 [2020-06-22]

- Updated dependencies.
