# 0.8.3 [unreleased]

- Fix a regression resulting in a panic with the `V1Lazy` protocol.
  [PR 1783](https://github.com/libp2p/rust-libp2p/pull/1783).

- Fix a potential deadlock during protocol negotiation due
  to a missing flush, potentially resulting in sporadic protocol
  upgrade timeouts.
  [PR 1781](https://github.com/libp2p/rust-libp2p/pull/1781).

- Update dependencies.

# 0.8.2 [2020-06-22]

- Updated dependencies.
