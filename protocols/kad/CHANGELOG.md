## 0.47.1

- Expose Distance private field U256 to public.
  See [PR 5705](https://github.com/libp2p/rust-libp2p/pull/5705).
- Fix systematic memory allocation when iterating over `KBuckets`.
  See [PR 5715](https://github.com/libp2p/rust-libp2p/pull/5715).

## 0.47.0

- Expose a kad query facility allowing specify num_results dynamicaly.
  See [PR 5555](https://github.com/libp2p/rust-libp2p/pull/5555).
- Add `mode` getter on `Behaviour`.
  See [PR 5573](https://github.com/libp2p/rust-libp2p/pull/5573).
- Add `Behavior::find_closest_local_peers()`.
  See [PR 5645](https://github.com/libp2p/rust-libp2p/pull/5645).
- Fix `cargo clippy` warnings in `rustc 1.84.0-beta.1`.
  See [PR 5700](https://github.com/libp2p/rust-libp2p/pull/5700).

## 0.46.2

- Emit `ToSwarm::NewExternalAddrOfPeer`.
  See [PR 5549](https://github.com/libp2p/rust-libp2p/pull/5549)

## 0.46.1

- Use new provider record update strategy to prevent Sybil attack.
  See [PR 5536](https://github.com/libp2p/rust-libp2p/pull/5536).

## 0.46.0

- Included multiaddresses of found peers alongside peer IDs in `GetClosestPeers` query results.
  See [PR 5475](https://github.com/libp2p/rust-libp2p/pull/5475)
- Changed `FIND_NODE` response: now includes a list of closest peers when querying the recipient peer ID. Previously, this request yielded an empty response.
  See [PR 5270](https://github.com/libp2p/rust-libp2p/pull/5270).
- Update to DHT republish interval and expiration time defaults to 22h and 48h respectively, rationale in [libp2p/specs#451](https://github.com/libp2p/specs/pull/451).
  See [PR 3230](https://github.com/libp2p/rust-libp2p/pull/3230).
- Use default dial conditions more consistently.
  See [PR 4957](https://github.com/libp2p/rust-libp2p/pull/4957)
- QueryClose progress whenever closer in range, instead of having to be the closest.
  See [PR 4934](https://github.com/libp2p/rust-libp2p/pull/4934).
- Add periodic and automatic bootstrap.
  See [PR 4838](https://github.com/libp2p/rust-libp2p/pull/4838).
- Make it mandatory to provide protocol names when creating a `kad::Config`.
  Deprecate `kad::Config::default()`, replaced by `kad::Config::new(StreamProtocol)`.
  See [PR 5122](https://github.com/libp2p/rust-libp2p/pull/5122).
- Compute `jobs_query_capacity` accurately.
  See [PR 5148](https://github.com/libp2p/rust-libp2p/pull/5148).
- Derive `Copy` for `kbucket::key::Key<T>`.
  See [PR 5317](https://github.com/libp2p/rust-libp2p/pull/5317).
⁻ `KBucket` size can now be modified without changing the `K_VALUE`.
  See [PR 5414](https://github.com/libp2p/rust-libp2p/pull/5414).
- Use `web-time` instead of `instant`.
  See [PR 5347](https://github.com/libp2p/rust-libp2p/pull/5347).
<!-- Update to libp2p-swarm v0.45.0 -->
- Correctly handle the `NoKnownPeers` error on automatic bootstrap.
  See [PR 5349](https://github.com/libp2p/rust-libp2p/pull/5349).
- Improve automatic bootstrap triggering conditions:
  trigger when the routing table is updated and we have less that `K_VALUE` peers in it,
  trigger when a new listen address is discovered and we have no connected peers.
  See [PR 5474](https://github.com/libp2p/rust-libp2p/pull/5474).

## 0.45.3

- The progress of the close query iterator shall be decided by ANY of the new peers.
  See [PR 4932](https://github.com/libp2p/rust-libp2p/pull/4932).

## 0.45.2

- Ensure `Multiaddr` handled and returned by `Behaviour` are `/p2p` terminated.
  See [PR 4596](https://github.com/libp2p/rust-libp2p/pull/4596).

## 0.45.1

- Fix a bug where calling `Behaviour::remove_address` with an address not in the peer's bucket would remove the peer from the routing table if the bucket has only one address left.
  See [PR 4816](https://github.com/libp2p/rust-libp2p/pull/4816)
- Add `std::fmt::Display` implementation on `QueryId`.
  See [PR 4814](https://github.com/libp2p/rust-libp2p/pull/4814).

## 0.45.0

- Remove deprecated `kad::Config::set_connection_idle_timeout` in favor of `SwarmBuilder::idle_connection_timeout`.
  See [PR 4659](https://github.com/libp2p/rust-libp2p/pull/4659).
- Emit `ModeChanged` event whenever we automatically reconfigure the mode.
  See [PR 4503](https://github.com/libp2p/rust-libp2p/pull/4503).
- Make previously "deprecated" `record` module private.
  See [PR 4035](https://github.com/libp2p/rust-libp2p/pull/4035).
- Expose hashed bytes of KBucketKey.
  See [PR 4698](https://github.com/libp2p/rust-libp2p/pull/4698).
- Remove previously deprecated type-aliases.
  Users should follow the convention of importing the `libp2p::kad` module and referring to symbols as `kad::Behaviour` etc.
  See [PR 4733](https://github.com/libp2p/rust-libp2p/pull/4733).

## 0.44.6

- Rename `Kademlia` symbols to follow naming convention.
  See [PR 4547].
- Fix a bug where we didn't detect a remote peer moving into client-state.
  See [PR 4639](https://github.com/libp2p/rust-libp2p/pull/4639).
- Re-export `NodeStatus`.
  See [PR 4645].
- Deprecate `kad::Config::set_connection_idle_timeout` in favor of `SwarmBuilder::idle_connection_timeout`.
  See [PR 4675].

[PR 4547]: https://github.com/libp2p/rust-libp2p/pull/4547
[PR 4645]: https://github.com/libp2p/rust-libp2p/pull/4645
[PR 4675]: https://github.com/libp2p/rust-libp2p/pull/4675

<!-- Internal changes

- Allow deprecated usage of `KeepAlive::Until`

-->

## 0.44.5

- Migrate to `quick-protobuf-codec` crate for codec logic.
  See [PR 4501].

[PR 4501]: https://github.com/libp2p/rust-libp2p/pull/4501

## 0.44.4

- Implement common traits on `RoutingUpdate`.
  See [PR 4270].
- Reduce noise of "remote supports our protocol" log.
  See [PR 4278].

[PR 4270]: https://github.com/libp2p/rust-libp2p/pull/4270
[PR 4278]: https://github.com/libp2p/rust-libp2p/pull/4278

## 0.44.3

- Prevent simultaneous dials to peers.
  See [PR 4224].

[PR 4224]: https://github.com/libp2p/rust-libp2p/pull/4224

- Rename missed `KademliaEvent::OutboundQueryCompleted` to `KademliaEvent::OutboundQueryProgressed` in documentation.
  See [PR 4257].

[PR 4257]: https://github.com/libp2p/rust-libp2p/pull/4257

## 0.44.2

- Allow to explicitly set `Mode::{Client,Server}`.
  See [PR 4132]

[PR 4132]: https://github.com/libp2p/rust-libp2p/pull/4132

## 0.44.1

- Expose `KBucketDistance`.
  See [PR 4109].

[PR 4109]: https://github.com/libp2p/rust-libp2p/pull/4109

## 0.44.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Remove deprecated public modules `handler`, `protocol` and `kbucket`.
  See [PR 3896].

- Automatically configure client/server mode based on external addresses.
  If we have or learn about an external address of our node, e.g. through `Swarm::add_external_address` or automated through `libp2p-autonat`, we operate in server-mode and thus allow inbound requests.
  By default, a node is in client-mode and only allows outbound requests.
  If you want to maintain the status quo, i.e. always operate in server mode, make sure to add at least one external address through `Swarm::add_external_address`.
  See also [Kademlia specification](https://github.com/libp2p/specs/tree/master/kad-dht#client-and-server-mode) for an introduction to Kademlia client/server mode.
  See [PR 3877].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3877]: https://github.com/libp2p/rust-libp2p/pull/3877
[PR 3896]: https://github.com/libp2p/rust-libp2p/pull/3896

## 0.43.3

- Preserve existing `KeepAlive::Until` timeout instead of continuously setting new `KeepAlive::Until(Instant::now() + self.config.idle_timeout)`.
  See [PR 3801].

[PR 3801]: https://github.com/libp2p/rust-libp2p/pull/3801

## 0.43.2

- Export pub enum `RoutingUpdate`. See [PR 3739].
- Deprecate `handler`, `kbucket`, `protocol`, `record` modules to make them private. See [PR 3738].

[PR 3739]: https://github.com/libp2p/rust-libp2p/pull/3739
[PR 3738]: https://github.com/libp2p/rust-libp2p/pull/3738

## 0.43.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.43.0

- Update to `libp2p-core` `v0.39.0`.

- Update to `libp2p-swarm` `v0.42.0`.

- Remove lifetime from `RecordStore` and use GATs instead. See [PR 3239].

- Limit number of active outbound streams to 32. See [PR 3287].

- Bump MSRV to 1.65.0.

[PR 3239]: https://github.com/libp2p/rust-libp2p/pull/3239
[PR 3287]: https://github.com/libp2p/rust-libp2p/pull/3287

## 0.42.1

- Skip unparsable multiaddr in `Peer::addrs`. See [PR 3280].

[PR 3280]: https://github.com/libp2p/rust-libp2p/pull/3280

## 0.42.0

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Replace `Kademlia`'s `NetworkBehaviour` implementation `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Replace `KademliaHandler`'s `ConnectionHandler` implementation `inject_*` methods with the new `on_*` methods.
  See [PR 3085].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

- Fix bad state transition on incoming `AddProvider` messages.
  This would eventually lead to warning that says: "New inbound substream to PeerId exceeds inbound substream limit. No older substream waiting to be reused."
  See [PR 3152].

- Refactor APIs to be streaming.
  - Renamed `KademliaEvent::OutboundQueryCompleted` to `KademliaEvent::OutboundQueryProgressed`
  - Instead of a single event `OutboundQueryCompleted`, there are now multiple events emitted, allowing the user to process them as they come in (via the new `OutboundQueryProgressed`). See `ProgressStep` to identify the final `OutboundQueryProgressed` of a single query.
  - To finish a query early, i.e. before the final `OutboundQueryProgressed` of the query, a caller needs to call `query.finish()`.
  - There is no more automatic caching of records. The user has to manually call `put_record_to` on the `QueryInfo::GetRecord.cache_candidates` to cache a record to a close peer that did not return the record on the foregone query.
    See [PR 2712].

[PR 3085]: https://github.com/libp2p/rust-libp2p/pull/3085
[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090
[PR 3152]: https://github.com/libp2p/rust-libp2p/pull/3152
[PR 2712]: https://github.com/libp2p/rust-libp2p/pull/2712

## 0.41.0

- Remove deprecated `set_protocol_name()` from `KademliaConfig` & `KademliaProtocolConfig`.
  Use `set_protocol_names()` instead. See [PR 2866].

- Bump rand to 0.8 and quickcheck to 1. See [PR 2857].

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

[PR 2866]: https://github.com/libp2p/rust-libp2p/pull/2866
[PR 2857]: https://github.com/libp2p/rust-libp2p/pull/2857

## 0.40.0

- Add support for multiple protocol names. Update `Kademlia`, `KademliaConfig`,
  and `KademliaProtocolConfig` accordingly. See [Issue 2837]. See [PR 2846].

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

[Issue 2837]: https://github.com/libp2p/rust-libp2p/issues/2837
[PR 2846]: https://github.com/libp2p/rust-libp2p/pull/2846

## 0.39.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-core` `v0.35.0`.

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.38.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

## 0.37.1

- Limit # of inbound streams to 32. [See PR 2699].

[PR 2699]: https://github.com/libp2p/rust-libp2p/pull/2699

## 0.37.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- Derive `Error` for `GetRecordError` (see [PR 2614]).

[PR 2614]: https://github.com/libp2p/rust-libp2p/pull/2614

## 0.36.0

- Update to `libp2p-swarm` `v0.35.0`.

## 0.35.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Require owned key in `get_record()` method (see [PR 2477]).

- Merge NetworkBehaviour's inject\_\* paired methods (see PR 2445).

[PR 2477]: https://github.com/libp2p/rust-libp2p/pull/2477
[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

## 0.34.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- Derive `Clone` for `KademliaEvent` (see [PR 2411])

- Derive `Serialize`, `Deserialize` for `store::record::Key` (see [PR 2408])

- Add `get_closest_local_peers` to `Kademlia` (see [PR 2436])

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2411]: https://github.com/libp2p/rust-libp2p/pull/2411
[PR 2408]: https://github.com/libp2p/rust-libp2p/pull/2408
[PR 2436]: https://github.com/libp2p/rust-libp2p/pull/2436

## 0.33.0 [2021-11-16]

- Use `instant` and `futures-timer` instead of `wasm-timer` (see [PR 2245]).

- Rename `KademliaEvent::InboundRequestServed` to `KademliaEvent::InboundRequest` and move
  `InboundPutRecordRequest` into `InboundRequest::PutRecord` and `InboundAddProviderRequest` into
  `InboundRequest::AddProvider` (see [PR 2297]).

- Populate the `key` field when converting `KadRequestMsg::PutValue` to `proto::Message` (see [PR 2309]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245
[PR 2297]: https://github.com/libp2p/rust-libp2p/pull/2297
[PR 2309]: https://github.com/libp2p/rust-libp2p/pull/2309

## 0.32.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Introduce `KademliaStoreInserts` option, which allows to filter records (see
  [PR 2163]).

- Check local store when calling `Kademlia::get_providers` (see [PR 2221]).

[PR 2163]: https://github.com/libp2p/rust-libp2p/pull/2163
[PR 2221]: https://github.com/libp2p/rust-libp2p/pull/2163

## 0.31.0 [2021-07-12]

- Update dependencies.

- Expose inbound request information (see [PR 2087]). Note:
  `KademliaEvent::QueryResult` is renamed to
  `KademliaEvent::OutboundQueryCompleted`.

- Expose whether `KademliaEvent::RoutingUpdated` is triggered with new peer (see
  [PR 2087]).

- Expose kbucket range on `KademliaEvent::RoutingUpdated` (see [PR 2087]).

- Remove false `debug_assert` on `connected_peers` (see [PR 2120]).

- Return correct number of remaining bootstrap requests (see [PR 2125]).

[PR 2087]: https://github.com/libp2p/rust-libp2p/pull/2087
[PR 2120]: https://github.com/libp2p/rust-libp2p/pull/2120
[PR 2125]: https://github.com/libp2p/rust-libp2p/pull/2125

## 0.30.0 [2021-04-13]

- Update `libp2p-swarm`.

## 0.29.0 [2021-03-17]

- Add `KademliaCaching` and `KademliaConfig::set_caching` to configure
  whether Kademlia should track, in lookups, the closest nodes to a key
  that did not return a record, via `GetRecordOk::cache_candidates`.
  As before, if a lookup used a quorum of 1, these candidates will
  automatically be sent the found record. Otherwise, with a lookup
  quorum of > 1, the candidates can be used with `Kademlia::put_record_to`
  after selecting one of the return records to cache. As is the current
  behaviour, caching is enabled by default with a `max_peers` of 1, i.e.
  it only tracks the closest node to the key that did not return a record.

- Add `Kademlia::put_record_to` for storing a record at specific nodes,
  e.g. for write-back caching after a successful read with quorum > 1.

- Update `libp2p-swarm`.

- Update dependencies.

## 0.28.1 [2021-02-15]

- Update dependencies.

## 0.28.0 [2021-01-12]

- Update dependencies.

## 0.27.1 [2021-01-11]

- Add From impls for `kbucket::Key`.
  [PR 1909](https://github.com/libp2p/rust-libp2p/pull/1909).

## 0.27.0 [2020-12-17]

- Update `libp2p-core` and `libp2p-swarm`.

## 0.26.0 [2020-11-25]

- Update `libp2p-core` and `libp2p-swarm`.

- Have two `ProviderRecord`s be equal iff their `key` and `provider` fields are
  equal. [PR 1850](https://github.com/libp2p/rust-libp2p/pull/1850/).

## 0.25.0 [2020-11-09]

- Upon newly established connections, delay routing table
  updates until after the configured protocol name has
  been confirmed by the connection handler, i.e. until
  after at least one substream has been successfully
  negotiated. In configurations with different protocol names,
  this avoids undesirable nodes being included in the
  local routing table at least temporarily.
  [PR 1821](https://github.com/libp2p/rust-libp2p/pull/1821).

- Update dependencies.

## 0.24.0 [2020-10-16]

- Update `libp2p-core` and `libp2p-swarm`.

- Update `sha2` dependency.

## 0.23.0 [2020-09-09]

- Increase default max packet size from 4KiB to 16KiB.
  See [issue 1622](https://github.com/libp2p/rust-libp2p/issues/1622).

- Add `Distance::log2` ([PR 1719](https://github.com/libp2p/rust-libp2p/pull/1719)).

- Update `libp2p-swarm` and `libp2p-core`.

## 0.22.1 [2020-08-19]

- Explicitly convert from u8 to usize in `BucketIndex::range` to prevent type
  inference issues ([PR 1716](https://github.com/libp2p/rust-libp2p/pull/1716)).

## 0.22.0 [2020-08-18]

- Store addresses in provider records.
  See [PR 1708](https://github.com/libp2p/rust-libp2p/pull/1708).

- Update `libp2p-core` and `libp2p-swarm` dependencies.

- Add `KBucketRef::range` exposing the minimum inclusive and maximum inclusive
  `Distance` for the bucket
  ([PR 1680](https://github.com/libp2p/rust-libp2p/pull/1680)).

- Add `NetworkBehaviour::inject_address_change` implementation
  ([PR 1649](https://github.com/libp2p/rust-libp2p/pull/1649)).

## 0.21.0 [2020-07-01]

- Remove `KademliaEvent::Discovered`
  ([PR 1632](https://github.com/libp2p/rust-libp2p/pull/1632))

- More control and insight for k-buckets
  ([PR 1628](https://github.com/libp2p/rust-libp2p/pull/1628)).
  In particular, `Kademlia::kbuckets_entries` has been removed and
  replaced by `Kademlia::kbuckets`/`Kademlia::kbucket` which provide
  more information than just the peer IDs. Furthermore `Kademlia::add_address`
  now returns a result and two new events, `KademliaEvent::RoutablePeer`
  and `KademliaEvent::PendingRoutablePeer` are introduced (but are not
  required to be acted upon in order to retain existing behaviour).
  For more details, see the PR description.

## 0.20.1 [2020-06-23]

- Maintenance release ([PR 1623](https://github.com/libp2p/rust-libp2p/pull/1623)).

## 0.20.0 [2020-06-22]

- Optionally require iterative queries to use disjoint paths based
  on S/Kademlia for increased resiliency in the presence of potentially
  adversarial nodes ([PR 1473](https://github.com/libp2p/rust-libp2p/pull/1473)).

- Re-export several types
  ([PR 1595](https://github.com/libp2p/rust-libp2p/pull/1595)).

- Updated dependencies.
