## 0.4.2

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

## 0.4.1

- Add getters & setters for the allowed/blocked peers.
  Return a `bool` for every "insert/remove" function, informing if a change was performed.
  See [PR 5572](https://github.com/libp2p/rust-libp2p/pull/5572).

## 0.4.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.3.0


## 0.2.0

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.1.1

- Correctly unblock and disallow peer in `unblock_peer` and `disallow_peer` functions.
  See [PR 3789].

[PR 3789]: https://github.com/libp2p/rust-libp2p/pull/3789

## 0.1.0

- Initial release.
