## 0.46.1
- Discard `Info`s received from remote peers that contain a public key that doesn't match their peer ID.
  See [PR 5707](https://github.com/libp2p/rust-libp2p/pull/5707).

## 0.46.0

- Make `identify::Config` fields private and add getter functions.
  See [PR 5663](https://github.com/libp2p/rust-libp2p/pull/5663).

## 0.45.1

- Add `hide_listen_addrs` option to prevent leaking (local) listen addresses.
  See [PR 5507](https://github.com/libp2p/rust-libp2p/pull/5507).

## 0.45.0

- Address translation is moved here from `libp2p-core`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)

<!-- Update to libp2p-swarm v0.45.0 -->

- Add `ConnectionId` in `Event`.
  See [PR 4981](https://github.com/libp2p/rust-libp2p/pull/4981).

## 0.44.2

- Emit `ToSwarm::NewExternalAddrOfPeer` for all external addresses of remote peers.
  For this work, the address cache must be enabled via `identify::Config::with_cache_size`.
  The default is 0, i.e. disabled.
  See [PR 4371](https://github.com/libp2p/rust-libp2p/pull/4371).

## 0.44.1

- Ensure `Multiaddr` handled and returned by `Behaviour` are `/p2p` terminated.
  See [PR 4596](https://github.com/libp2p/rust-libp2p/pull/4596).

## 0.44.0

- Add `Info` to the `libp2p-identify::Event::Pushed` to report pushed info.
  See [PR 4527](https://github.com/libp2p/rust-libp2p/pull/4527)
- Remove deprecated `initial_delay`.
  Identify requests are always sent instantly after the connection has been established.
  See [PR 4735](https://github.com/libp2p/rust-libp2p/pull/4735)
- Don't repeatedly report the same observed address as a `NewExternalAddrCandidate`.
  Instead, only report each observed address once per connection.
  This allows users to probabilistically deem an address as external if it gets reported as a candidate repeatedly.
  See [PR 4721](https://github.com/libp2p/rust-libp2p/pull/4721).

## 0.43.1

- Handle partial push messages.
  Previously, push messages with partial information were ignored.
  See [PR 4495].

[PR 4495]: https://github.com/libp2p/rust-libp2p/pull/4495

## 0.43.0

- Observed addresses (aka. external address candidates) of the local node, reported by a remote node via `libp2p-identify`, are no longer automatically considered confirmed external addresses, in other words they are no longer trusted by default.
  Instead users need to confirm the reported observed address either manually, or by using `libp2p-autonat`.
  In trusted environments users can simply extract observed addresses from a `libp2p-identify::Event::Received { info: libp2p_identify::Info { observed_addr }}` and confirm them via `Swarm::add_external_address`.
  See [PR 3954] and [PR 4052].

- Remove deprecated `Identify` prefixed symbols. See [PR 3698].
- Raise MSRV to 1.65.
  See [PR 3715].

- Reduce the initial delay before running the identify protocol to 0 and make the option deprecated.
  See [PR 3545].

- Fix aborting the answering of an identify request in rare situations.
  See [PR 3876].

- Actively push changes in listen protocols to remote.
  See [PR 3980].

[PR 3545]: https://github.com/libp2p/rust-libp2p/pull/3545
[PR 3698]: https://github.com/libp2p/rust-libp2p/pull/3698
[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3876]: https://github.com/libp2p/rust-libp2p/pull/3876
[PR 3954]: https://github.com/libp2p/rust-libp2p/pull/3954
[PR 3980]: https://github.com/libp2p/rust-libp2p/pull/3980
[PR 4052]: https://github.com/libp2p/rust-libp2p/pull/4052

## 0.42.2

- Do not implicitly dial a peer upon `identify::Behaviour::push`.
  Previously, we would dial each peer in the provided list.
  Now, we skip peers that we are not connected to.
  See [PR 3843].

[PR 3843]: https://github.com/libp2p/rust-libp2p/pull/3843

## 0.42.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.42.0

- Update to `libp2p-core` `v0.39.0`.

- Move I/O from `Behaviour` to `Handler`. Handle `Behaviour`'s Identify and Push requests independently by incoming order,
  previously Push requests were prioritized. see [PR 3208].

- Update to `libp2p-swarm` `v0.42.0`.

- Don't close the stream when reading the identify info in `protocol::recv`. See [PR 3344].

[PR 3208]: https://github.com/libp2p/rust-libp2p/pull/3208
[PR 3344]: https://github.com/libp2p/rust-libp2p/pull/3344

## 0.41.1

- Skip invalid multiaddr in `listen_addrs`. See [PR 3246].

[PR 3246]: https://github.com/libp2p/rust-libp2p/pull/3246

## 0.41.0

- Change default `cache_size` of `Config` to 100. See [PR 2995].

- Update to `prost-codec` `v0.3.0`.

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Replace `Behaviour`'s `NetworkBehaviour` implemention `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Replace `Handler`'s `ConnectionHandler` implemention `inject_*` methods with the new `on_*` methods.
  See [PR 3085].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

[PR 3085]: https://github.com/libp2p/rust-libp2p/pull/3085
[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 2995]: https://github.com/libp2p/rust-libp2p/pull/2995
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.40.0

- Update dependencies.

- Rename types as per [discussion 2174].
  `Identify` has been renamed to `Behaviour`.
  The `Identify` prefix has been removed from various types like `IdentifyEvent`.
  Users should prefer importing the identify protocol as a module (`use libp2p::identify;`),
  and refer to its types via `identify::`. For example: `identify::Behaviour` or `identify::Event`.

  [discussion 2174]: https://github.com/libp2p/rust-libp2p/discussions/2174

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

## 0.39.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

## 0.38.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Expose `PROTOCOL_NAME` and `PUSH_PROTOCOL_NAME`. See [PR 2734].

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788
[PR 2734]: https://github.com/libp2p/rust-libp2p/pull/2734/

## 0.37.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

- Extend log message on second identify push stream with peer ID.

## 0.36.1

- Allow at most one inbound identify push stream.

## 0.36.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- Expose explicits errors via `UpgradeError` instead of generic `io::Error`. See [PR 2630].

[PR 2630]: https://github.com/libp2p/rust-libp2p/pull/2630
## 0.35.0

- Update to `libp2p-swarm` `v0.35.0`.

## 0.34.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

## 0.33.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.32.0 [2021-11-16]

- Use `futures-timer` instead of `wasm-timer` (see [PR 2245]).
- Filter invalid peers from cache used in `addresses_of_peer` – [PR 2338].

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245
[PR 2338]: https://github.com/libp2p/rust-libp2p/pull/2338

## 0.31.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Assist in peer discovery by optionally returning reported listen addresses
  of other peers from `addresses_of_peer` (see [PR
  2232](https://github.com/libp2p/rust-libp2p/pull/2232)), disabled by default.

## 0.30.0 [2021-07-12]

- Update dependencies.

## 0.29.0 [2021-04-13]

- Add support for configurable automatic push to connected peers
  on listen addr changes. Disabled by default.
  [PR 2004](https://github.com/libp2p/rust-libp2p/pull/2004)

- Implement the `/ipfs/id/push/1.0.0` protocol.
  cf. https://github.com/libp2p/specs/tree/master/identify#identifypush
  [PR 1999](https://github.com/libp2p/rust-libp2p/pull/1999)

- Emit `IdentifyEvent::Pushed` event after successfully pushing identification
  information to peer [PR
  2030](https://github.com/libp2p/rust-libp2p/pull/2030).

## 0.28.0 [2021-03-17]

- Update `libp2p-swarm`.

- Update dependencies.

## 0.27.0 [2021-01-12]

- Update dependencies.

## 0.26.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.25.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.24.0 [2020-11-09]

- Update dependencies.

## 0.23.0 [2020-10-16]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.22.0 [2020-09-09]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.21.0 [2020-08-18]

- Bump `libp2p-core` and `libp2p-swarm` dependencies.

## 0.20.0 [2020-07-01]

- Updated dependencies.

## 0.19.2 [2020-06-22]

- Updated dependencies.
