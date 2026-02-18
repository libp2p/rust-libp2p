## 0.50.0

- Add extra metrics for bytes received and sent, filtered and unfiltered for each topic.
  See [PR 6192](https://github.com/libp2p/rust-libp2p/pull/6192)

- Reduce log size by implementing custom Debug for RawMessage that logs data length instead of full byte arrays.
  See [PR 6263](https://github.com/libp2p/rust-libp2p/pull/6263)

- Log when sending and receiving messages.
  See [PR 6234](https://github.com/libp2p/rust-libp2p/pull/6234)

- Prevent mesh exceeding mesh_n_high.
  See [PR 6184](https://github.com/libp2p/rust-libp2p/pull/6184)

- Fix underflow when shuffling peers after prunning.
  See [PR 6183](https://github.com/libp2p/rust-libp2p/pull/6183)

- Remove peer penalty for duplicate messages.
  See [PR 6112](https://github.com/libp2p/rust-libp2p/pull/6112)

- Remove `Rpc` from the public API.
  See [PR 6091](https://github.com/libp2p/rust-libp2p/pull/6091)

- reduce allocations by replacing `or_insert` with `or_insert_with`
  See [PR 6136](https://github.com/libp2p/rust-libp2p/pull/6136)

- Fix `unsubscribe_backoff` expecting number of seconds instead of `Duration`
  See [PR 6124](https://github.com/libp2p/rust-libp2p/pull/6124)

- Fix incorrect default values in ConfigBuilder
  See [PR 6113](https://github.com/libp2p/rust-libp2p/pull/6113)

- Remove duplicated config `set_topic_max_transmit_size` method, prefer `max_transmit_size_for_topic`.
  See [PR 6173](https://github.com/libp2p/rust-libp2p/pull/6173).

- Switch the internal `async-channel` used to dispatch messages from `NetworkBehaviour` to the `ConnectionHandler`
  with an internal priority queue. See [PR 6175](https://github.com/libp2p/rust-libp2p/pull/6175)

- gossipsub: do early return in for an empty input
  See [PR 6208](https://github.com/libp2p/rust-libp2p/pull/6208).

- Refactor gossipsub with in-place negative-score peer removal.
  See [PR 6209](https://github.com/libp2p/rust-libp2p/pull/6209).

- Avoid direct casting from u128 to u64.
  See [PR 6211](https://github.com/libp2p/rust-libp2p/pull/6211).

## 0.49.2

- Relax `Behaviour::with_metrics` requirements, do not require DataTransform and TopicSubscriptionFilter to also impl Default
  See [PR 6097](https://github.com/libp2p/rust-libp2p/pull/6097)

## 0.49.1

- Fix applying P3 and P6 Score penalties when their weight is zero
  See [PR 6097](https://github.com/libp2p/rust-libp2p/pull/6097)

- Fix fanout logic to include correctly scored peers and prevent panics when memcache is set to 0.
  See [PR 6095](https://github.com/libp2p/rust-libp2p/pull/6095)

- Fix mesh not being constructed even when not adding any peer.
  See [PR 6100](https://github.com/libp2p/rust-libp2p/pull/6100)

## 0.49.0

- Feature gate metrics related code. This changes some `Behaviour` constructor methods.
  See [PR 6020](https://github.com/libp2p/rust-libp2p/pull/6020)

- Send IDONTWANT before Publishing a new message.
  See [PR 6017](https://github.com/libp2p/rust-libp2p/pull/6017)

- Improve log messaging by renaming `message` to `message_id`. This change makes it log coherent to
  the field and also improve duplication of `message` field.
  See [PR 5972](https://github.com/libp2p/rust-libp2p/pull/5972)

- Fix a race condition for messages published which are already in the network.
  See [PR 5928](https://github.com/libp2p/rust-libp2p/pull/5928)

- Improve error messaging by renaming `PublishError::InsufficientPeers` to
  `PublishError::NoPeersSubscribedToTopic`. This change makes it clearer that the error occurs
  specifically when trying to publish to a topic with no subscribed peers, rather than a general
  peer availability issue.
  See [PR 5912](https://github.com/libp2p/rust-libp2p/pull/5912)

- Allow whitelisting topics for metrics to ensure metrics are recorded correctly for these topics.
  See [PR 5895](https://github.com/libp2p/rust-libp2p/pull/5895)

- Improve `max_messages_per_rpc` consistency by ensuring RPC control messages also adhere to the existing limits.
  See [PR 5826](https://github.com/libp2p/rust-libp2p/pull/5826)

- Respect already received IDONTWANT messages when handling IWANT.
  See [PR 5901](https://github.com/libp2p/rust-libp2p/pull/5901)

- Fix messages were not forwarded to floodsub peers.
  See [PR 5908](https://github.com/libp2p/rust-libp2p/pull/5908)

- Fix messages were published to all floodsub peers regardless of their topic.
  See [PR 5904](https://github.com/libp2p/rust-libp2p/pull/5904)

- Upgrade `prometheus-client` to `v0.23`
  See [PR 5960](https://github.com/libp2p/rust-libp2p/pull/5960).

- Allow customizing max transmit size and mesh-n-* parameters per topic.
  See [PR 5868](https://github.com/libp2p/rust-libp2p/pull/5868)

<!-- Update to libp2p-swarm v0.47.0 -->

## 0.48.0

- Allow broadcasting `IDONTWANT` messages when publishing to avoid downloading data that is already available.
  See [PR 5773](https://github.com/libp2p/rust-libp2p/pull/5773)

- Add configurable `idontwant_message_size_threshold` parameter.
  See [PR 5770](https://github.com/libp2p/rust-libp2p/pull/5770)

- Introduce Gossipsub v1.2 [spec](https://github.com/libp2p/specs/blob/master/pubsub/gossipsub/gossipsub-v1.2.md).
  See [PR 5697](https://github.com/libp2p/rust-libp2p/pull/5697)

- Correct state inconsistencies with the mesh and fanout when unsubscribing.
  See [PR 5690](https://github.com/libp2p/rust-libp2p/pull/5690)

- Deprecate `futures-ticker` and use `futures-timer` instead.
  See [PR 5674](https://github.com/libp2p/rust-libp2p/pull/5674).

- Apply `max_transmit_size` to the inner message instead of the final payload.
  See [PR 5642](https://github.com/libp2p/rust-libp2p/pull/5642).

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

- Attempt to publish to at least mesh_n peers when flood publish is disabled.
  See [PR 5578](https://github.com/libp2p/rust-libp2p/pull/5578).

- Introduce back pressure and penalize slow peers. Drop stale messages that timeout before being
  delivered.
  See [PR 5595](https://github.com/libp2p/rust-libp2p/pull/5595).

- Change `Behaviour::unsubscribe` and `Behaviour::report_message_validation_result`
  to `bool` they don't need to be a `Result`.
  See [PR 5595](https://github.com/libp2p/rust-libp2p/pull/5595).

- Fix `cargo clippy` warnings in `rustc 1.84.0-beta.1`.
  See [PR 5700](https://github.com/libp2p/rust-libp2p/pull/5700).

- Fix an issue where an `InsufficientPeers` error could occur under certain conditions, despite having peers subscribed to a topic.
  See [PR 5793](https://github.com/libp2p/rust-libp2p/pull/5793).

<!-- Update to libp2p-core v0.43.0 -->

## 0.47.0

<!-- Update to libp2p-swarm v0.45.0 -->
- Add ConnectionError to FromSwarm::ConnectionClosed.
  See [PR 5485](https://github.com/libp2p/rust-libp2p/pull/5485).

## 0.46.2
- Use `web-time` instead of `instant`.
  See [PR 5347](https://github.com/libp2p/rust-libp2p/pull/5347).

## 0.46.1

- Deprecate `Rpc` in preparation for removing it from the public API because it is an internal type.
  See [PR 4833](https://github.com/libp2p/rust-libp2p/pull/4833).

## 0.46.0

- Remove `fast_message_id_fn` mechanism from `Config`.
  See [PR 4285](https://github.com/libp2p/rust-libp2p/pull/4285).
- Remove deprecated `gossipsub::Config::idle_timeout` in favor of `SwarmBuilder::idle_connection_timeout`.
  See [PR 4642](https://github.com/libp2p/rust-libp2p/pull/4642).
- Return typed error from config builder.
  See [PR 4445](https://github.com/libp2p/rust-libp2p/pull/4445).
- Process outbound stream before inbound stream in `EnabledHandler::poll(..)`.
  See [PR 4778](https://github.com/libp2p/rust-libp2p/pull/4778).

## 0.45.2

- Deprecate `gossipsub::Config::idle_timeout` in favor of `SwarmBuilder::idle_connection_timeout`.
  See [PR 4648].

<!-- Internal changes:

- Allow new clippy lint.

-->

[PR 4648]: (https://github.com/libp2p/rust-libp2p/pull/4648)

<!-- Internal changes

- Allow deprecated usage of `KeepAlive::Until`

-->

## 0.45.1

- Add getter function to obtain `TopicScoreParams`.
  See [PR 4231].

[PR 4231]: https://github.com/libp2p/rust-libp2p/pull/4231

## 0.45.0

- Raise MSRV to 1.65.
  See [PR 3715].
- Remove deprecated items. See [PR 3862].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3862]: https://github.com/libp2p/rust-libp2p/pull/3862

## 0.44.4

- Deprecate `metrics`, `protocol`, `subscription_filter`, `time_cache` modules to make them private. See [PR 3777].
- Honor the `gossipsub::Config::support_floodsub` in all cases.
  Previously, it was ignored when a custom protocol id was set via `gossipsub::Config::protocol_id`.
  See [PR 3837].

[PR 3777]: https://github.com/libp2p/rust-libp2p/pull/3777
[PR 3837]: https://github.com/libp2p/rust-libp2p/pull/3837

## 0.44.3

- Fix erroneously duplicate message IDs. See [PR 3716].

- Gracefully disable handler on stream errors. Deprecate a few variants of `HandlerError`.
  See [PR 3625].

[PR 3716]: https://github.com/libp2p/rust-libp2p/pull/3716
[PR 3625]: https://github.com/libp2p/rust-libp2p/pull/3325

## 0.44.2

- Signed messages now use sequential integers in the sequence number field.
  See [PR 3551].

[PR 3551]: https://github.com/libp2p/rust-libp2p/pull/3551

## 0.44.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.44.0

- Update to `prometheus-client` `v0.19.0`. See [PR 3207].

- Update to `libp2p-core` `v0.39.0`.

- Update to `libp2p-swarm` `v0.42.0`.

- Initialize `ProtocolConfig` via `GossipsubConfig`. See [PR 3381].

- Rename types as per [discussion 2174].
  `Gossipsub` has been renamed to `Behaviour`.
  The `Gossipsub` prefix has been removed from various types like `GossipsubConfig` or `GossipsubMessage`.
  It is preferred to import the gossipsub protocol as a module (`use libp2p::gossipsub;`), and refer to its types via `gossipsub::`.
  For example: `gossipsub::Behaviour` or `gossipsub::RawMessage`. See [PR 3303].

[PR 3207]: https://github.com/libp2p/rust-libp2p/pull/3207/
[PR 3303]: https://github.com/libp2p/rust-libp2p/pull/3303/
[PR 3381]: https://github.com/libp2p/rust-libp2p/pull/3381/
[discussion 2174]: https://github.com/libp2p/rust-libp2p/discussions/2174

## 0.43.0

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Update to `prost-codec` `v0.3.0`.

- Refactoring GossipsubCodec to use common protobuf Codec. See [PR 3070].

- Replace `Gossipsub`'s `NetworkBehaviour` implementation `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Replace `GossipsubHandler`'s `ConnectionHandler` implementation `inject_*` methods with the new `on_*` methods.
  See [PR 3085].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

[PR 3085]: https://github.com/libp2p/rust-libp2p/pull/3085
[PR 3070]: https://github.com/libp2p/rust-libp2p/pull/3070
[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.42.0

- Bump rand to 0.8 and quickcheck to 1. See [PR 2857].

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

[PR 2857]: https://github.com/libp2p/rust-libp2p/pull/2857

## 0.41.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

- Allow publishing with any `impl Into<TopicHash>` as a topic. See [PR 2862].

[PR 2862]: https://github.com/libp2p/rust-libp2p/pull/2862

## 0.40.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-core` `v0.35.0`.

- Update to `prometheus-client` `v0.18.0`. See [PR 2822].

[PR 2822]: https://github.com/libp2p/rust-libp2p/pull/2761/
[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.39.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

- Allow for custom protocol ID via `GossipsubConfigBuilder::protocol_id()`. See [PR 2718].

[PR 2718]: https://github.com/libp2p/rust-libp2p/pull/2718/

## 0.38.1

- Fix duplicate connection id. See [PR 2702].

[PR 2702]: https://github.com/libp2p/rust-libp2p/pull/2702

## 0.38.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- changed `TimeCache::contains_key` and `DuplicateCache::contains` to immutable methods. See [PR 2620].

- Update to `prometheus-client` `v0.16.0`. See [PR 2631].

[PR 2620]: https://github.com/libp2p/rust-libp2p/pull/2620
[PR 2631]: https://github.com/libp2p/rust-libp2p/pull/2631

## 0.37.0

- Update to `libp2p-swarm` `v0.35.0`.

- Fix gossipsub metric (see [PR 2558]).

- Allow the user to set the buckets for the score histogram, and to adjust them from the score thresholds. See [PR 2595].

[PR 2558]: https://github.com/libp2p/rust-libp2p/pull/2558
[PR 2595]: https://github.com/libp2p/rust-libp2p/pull/2595

## 0.36.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Move from `open-metrics-client` to `prometheus-client` (see [PR 2442]).

- Emit gossip of all non-empty topics (see [PR 2481]).

- Merge NetworkBehaviour's inject_\* paired methods (see [PR 2445]).

- Revert to wasm-timer (see [PR 2506]).

- Do not overwrite msg's peers if put again into mcache (see [PR 2493]).

[PR 2442]: https://github.com/libp2p/rust-libp2p/pull/2442
[PR 2481]: https://github.com/libp2p/rust-libp2p/pull/2481
[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445
[PR 2506]: https://github.com/libp2p/rust-libp2p/pull/2506
[PR 2493]: https://github.com/libp2p/rust-libp2p/pull/2493

## 0.35.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- Add metrics for network and configuration performance analysis (see [PR 2346]).

- Improve bandwidth performance by tracking IWANTs and reducing duplicate sends
  (see [PR 2327]).

- Implement `Serialize` and `Deserialize` for `MessageId` and `FastMessageId` (see [PR 2408])

- Fix `GossipsubConfigBuilder::build()` requiring `&self` to live for `'static` (see [PR 2409])

- Implement Unsubscribe backoff as per [libp2p specs PR 383] (see [PR 2403]).

[PR 2346]: https://github.com/libp2p/rust-libp2p/pull/2346
[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2327]: https://github.com/libp2p/rust-libp2p/pull/2327
[PR 2408]: https://github.com/libp2p/rust-libp2p/pull/2408
[PR 2409]: https://github.com/libp2p/rust-libp2p/pull/2409
[PR 2403]: https://github.com/libp2p/rust-libp2p/pull/2403
[libp2p specs PR 383]: https://github.com/libp2p/specs/pull/383

## 0.34.0 [2021-11-16]

- Add topic and mesh metrics (see [PR 2316]).

- Fix bug in internal peer's topics tracking (see [PR 2325]).

- Use `instant` and `futures-timer` instead of `wasm-timer` (see [PR 2245]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245
[PR 2325]: https://github.com/libp2p/rust-libp2p/pull/2325
[PR 2316]: https://github.com/libp2p/rust-libp2p/pull/2316

## 0.33.0 [2021-11-01]

- Add an event to register peers that do not support the gossipsub protocol
  [PR 2241](https://github.com/libp2p/rust-libp2p/pull/2241)

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Improve internal peer tracking.
  [PR 2175](https://github.com/libp2p/rust-libp2p/pull/2175)

- Update dependencies.

- Allow `message_id_fn`s to accept closures that capture variables.
  [PR 2103](https://github.com/libp2p/rust-libp2p/pull/2103)

- Implement std::error::Error for error types.
  [PR 2254](https://github.com/libp2p/rust-libp2p/pull/2254)

## 0.32.0 [2021-07-12]

- Update dependencies.

- Reduce log levels across the crate to lessen noisiness of libp2p-gossipsub (see [PR 2101]).

[PR 2101]: https://github.com/libp2p/rust-libp2p/pull/2101

## 0.31.0 [2021-05-17]

- Keep connections to peers in a mesh alive. Allow closing idle connections to peers not in a mesh
  [PR-2043].

[PR-2043]: https://github.com/libp2p/rust-libp2p/pull/2043https://github.com/libp2p/rust-libp2p/pull/2043

## 0.30.1 [2021-04-27]

- Remove `regex-filter` feature flag thus always enabling `regex::RegexSubscriptionFilter` [PR
  2056](https://github.com/libp2p/rust-libp2p/pull/2056).

## 0.30.0 [2021-04-13]

- Update `libp2p-swarm`.

- Update dependencies.

## 0.29.0 [2021-03-17]

- Update `libp2p-swarm`.

- Update dependencies.

## 0.28.0 [2021-02-15]

- Prevent non-published messages being added to caches.
  [PR 1930](https://github.com/libp2p/rust-libp2p/pull/1930)

- Update dependencies.

## 0.27.0 [2021-01-12]

- Update dependencies.

- Implement Gossipsub v1.1 specification.
  [PR 1720](https://github.com/libp2p/rust-libp2p/pull/1720)

## 0.26.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.25.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.24.0 [2020-11-09]

- Update dependencies.

## 0.23.0 [2020-10-16]

- Update dependencies.

## 0.22.0 [2020-09-09]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.21.0 [2020-08-18]

- Add public API to list topics and peers. [PR 1677](https://github.com/libp2p/rust-libp2p/pull/1677).

- Add message signing and extended privacy/validation configurations. [PR 1583](https://github.com/libp2p/rust-libp2p/pull/1583).

- `Debug` instance for `Gossipsub`. [PR 1673](https://github.com/libp2p/rust-libp2p/pull/1673).

- Bump `libp2p-core` and `libp2p-swarm` dependency.

## 0.20.0 [2020-07-01]

- Updated dependencies.

## 0.19.3 [2020-06-23]

- Maintenance release fixing linter warnings.

## 0.19.2 [2020-06-22]

- Updated dependencies.
