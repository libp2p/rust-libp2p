# 0.9.0 [unreleased]

- Fix the encoding and decoding of `ls` responses to
  be spec-compliant and interoperable with other implementations.
  For a clean upgrade, `0.8.4` must already be deployed.

# 0.8.5 [2020-11-09]

- During negotiation do not interpret EOF error as an IO error, but instead as a
  negotiation error. See https://github.com/libp2p/rust-libp2p/pull/1823.

# 0.8.4 [2020-10-20]

- Temporarily disable the internal selection of "parallel" protocol
  negotiation for the dialer to later change the response format of the "ls"
  message for spec compliance. See https://github.com/libp2p/rust-libp2p/issues/1795.

# 0.8.3 [2020-10-16]

- Fix a regression resulting in a panic with the `V1Lazy` protocol.
  [PR 1783](https://github.com/libp2p/rust-libp2p/pull/1783).

- Fix a potential deadlock during protocol negotiation due
  to a missing flush, potentially resulting in sporadic protocol
  upgrade timeouts.
  [PR 1781](https://github.com/libp2p/rust-libp2p/pull/1781).

- Update dependencies.

# 0.8.2 [2020-06-22]

- Updated dependencies.
