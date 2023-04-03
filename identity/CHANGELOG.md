## 0.2.0 - unreleased

- Remove `PeerId::try_from_multiaddr`.
  `multiaddr::Protocol::P2p` is now type-safe and contains a `PeerId` directly, rendering this function obsolete.
  See [PR 3656].

[PR 3656]: https://github.com/libp2p/rust-libp2p/pull/3656

## 0.1.1

- Add `From` impl for specific keypairs.
  See [PR 3626].

[PR 3626]: https://github.com/libp2p/rust-libp2p/pull/3626
