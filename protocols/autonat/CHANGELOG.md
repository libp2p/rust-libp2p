## 0.13.2

- Update to `libp2p-request-response` `v0.28.0`.

## 0.13.1

- Verify that an incoming AutoNAT dial comes from a connected peer. See [PR 5597](https://github.com/libp2p/rust-libp2p/pull/5597).

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

## 0.13.0

- Due to the refactor of `Transport` it's no longer required to create a seperate transport for
AutoNAT where port reuse is disabled. This information is now passed by the behaviour.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568).
- Introduce the new AutoNATv2 protocol.
  It's split into a client and a server part, represented in their respective modules
  Features:
    - The server now always dials back over a newly allocated port.
      This more accurately reflects the reachability state for other peers and avoids accidental hole punching.
    - The server can now test addresses different from the observed address (i.e., the connection to the server was made through a `p2p-circuit`). To mitigate against DDoS attacks, the client has to send more data to the server than the dial-back costs.
  See [PR 5526](https://github.com/libp2p/rust-libp2p/pull/5526).

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.12.1
- Use `web-time` instead of `instant`.
  See [PR 5347](https://github.com/libp2p/rust-libp2p/pull/5347).

## 0.12.0

- Remove `Clone`, `PartialEq` and `Eq` implementations on `Event` and its sub-structs.
  The `Event` also contains errors which are not clonable or comparable.
  See [PR 3914](https://github.com/libp2p/rust-libp2p/pull/3914).

## 0.11.0

- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.10.2

- Store server `PeerId`s in `HashSet` to avoid duplicates and lower memory consumption.
  See [PR 3736].

[PR 3736]: https://github.com/libp2p/rust-libp2p/pull/3736

## 0.10.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3153]: https://github.com/libp2p/rust-libp2p/pull/3153

## 0.10.0

- Update to `libp2p-core` `v0.39.0`.

- Require the node's local `PeerId` to be passed into the constructor of `libp2p_autonat::Behaviour`. See [PR 3153].

- Update to `libp2p-request-response` `v0.24.0`.

- Update to `libp2p-swarm` `v0.42.0`.

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.9.1

- Skip unparsable multiaddr in `DialRequest::from_bytes`. See [PR 3351].

[PR 3351]: https://github.com/libp2p/rust-libp2p/pull/3351


## 0.9.0

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Update to `libp2p-request-response` `v0.23.0`.

- Replace `Behaviour`'s `NetworkBehaviour` implemention `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.8.0

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

- Update to `libp2p-request-response` `v0.22.0`.

## 0.7.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-request-response` `v0.21.0`.

- Update to `libp2p-core` `v0.36.0`.

## 0.6.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-request-response` `v0.20.0`.

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.5.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

- Update to `libp2p-request-response` `v0.19.0`.

## 0.4.1

- Export `DEFAULT_PROTOCOL_NAME`.

## 0.4.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- Update to `libp2p-request-response` `v0.18.0`.

- Add `Config::only_global_ips` to skip peers that are observed at a private IP-address
  (see [PR 2618]).

[PR 2618]: https://github.com/libp2p/rust-libp2p/pull/2618

## 0.3.0

- Update to `libp2p-swarm` `v0.35.0`.

- Update to `libp2p-request-response` `v0.17.0`.

## 0.2.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Update to `libp2p-request-response` `v0.16.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

## 0.1.0 [2022-01-27]

- Initial release.
