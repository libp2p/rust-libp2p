# 0.2.0 // unreleased

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

