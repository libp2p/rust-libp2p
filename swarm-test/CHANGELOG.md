## 0.6.0

- Default to `tokio` runtime.
  See [PR 6024](https://github.com/libp2p/rust-libp2p/pull/6024).
- Remove `async_std` runtime support with `Swarm::new_ephemeral`.
  Use `Swarm::new_ephemeral_tokio` instead.
  See [PR 6064](https://github.com/libp2p/rust-libp2p/pull/6064)

<!-- Update to libp2p-swarm v0.47.0 -->

## 0.5.0

- Add `tokio` runtime support and make `tokio` and `async-std` runtimes optional behind features.
  See [PR 5551].
  - Update default for idle-connection-timeout to 10s on `SwarmExt::new_ephemeral` methods.
  See [PR 4967](https://github.com/libp2p/rust-libp2p/pull/4967).

[PR 5551]: https://github.com/libp2p/rust-libp2p/pull/5551

## 0.4.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.3.0


## 0.2.0

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.1.0

- Initial release.
