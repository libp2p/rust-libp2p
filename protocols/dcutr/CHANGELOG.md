# 0.6.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

# 0.5.1

- Make default features of `libp2p-core` optional. See [PR 2836].

[PR 2836]: https://github.com/libp2p/rust-libp2p/pull/2836/

# 0.5.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Expose `PROTOCOL_NAME`. See [PR 2734].

- Update to `libp2p-core` `v0.35.0`.

[PR 2734]: https://github.com/libp2p/rust-libp2p/pull/2734/
[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

# 0.4.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

# 0.3.1

- Upgrade at most one inbound connect request.

# 0.3.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

# 0.2.0

- Expose `InboundUpgradeError` and `OutboundUpgradeError`. See [PR, 2586].

- Update to `libp2p-swarm` `v0.35.0`.

[PR 2586]: https://github.com/libp2p/rust-libp2p/pull/2586

# 0.1.0 [2022-02-22]

- Initial release.
