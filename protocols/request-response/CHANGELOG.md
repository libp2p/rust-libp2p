# 0.3.0 [unreleased]

- Add support for opt-in request-based flow-control to any
  request-response protocol via `RequestResponse::throttled()`.
  [PR 1726](https://github.com/libp2p/rust-libp2p/pull/1726).

- Update `libp2p-swarm` and `libp2p-core`.

# 0.2.0 [2020-08-18]

- Fixed connection keep-alive, permitting connections to close due
  to inactivity.
- Bump `libp2p-core` and `libp2p-swarm` dependencies.

# 0.1.1

- Always properly `close()` the substream after sending requests and
responses in the `InboundUpgrade` and `OutboundUpgrade`. Otherwise this is
left to `RequestResponseCodec::write_request` and `RequestResponseCodec::write_response`,
which can be a pitfall and lead to subtle problems (see e.g.
https://github.com/libp2p/rust-libp2p/pull/1606).

# 0.1.0

- Initial release.

