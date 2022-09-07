# 0.7.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-request-response` `v0.21.0`.

- Update to `libp2p-core` `v0.36.0`.

# 0.6.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-request-response` `v0.20.0`.

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

# 0.5.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

- Update to `libp2p-request-response` `v0.19.0`.

# 0.4.1

- Export `DEFAULT_PROTOCOL_NAME`.

# 0.4.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- Update to `libp2p-request-response` `v0.18.0`.

- Add `Config::only_global_ips` to skip peers that are observed at a private IP-address
  (see [PR 2618]).

[PR 2618]: https://github.com/libp2p/rust-libp2p/pull/2618

# 0.3.0

- Update to `libp2p-swarm` `v0.35.0`.

- Update to `libp2p-request-response` `v0.17.0`.

# 0.2.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Update to `libp2p-request-response` `v0.16.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

# 0.1.0 [2022-01-27]

- Initial release.
