## 0.15.0
- Use `web-time` instead of `instant`.
  See [PR 5347](https://github.com/libp2p/rust-libp2p/pull/5347).

## 0.14.1

- Add `BandwidthTransport`, wrapping an existing `Transport`, exposing Prometheus bandwidth metrics.
  See also `SwarmBuilder::with_bandwidth_metrics`.
  See [PR 4727](https://github.com/libp2p/rust-libp2p/pull/4727).

## 0.14.0

- Add metrics for `SwarmEvent::{NewExternalAddrCandidate,ExternalAddrConfirmed,ExternalAddrExpired}`.
  See [PR 4721](https://github.com/libp2p/rust-libp2p/pull/4721).

## 0.13.1

- Enable gossipsub related data-type fields when compiling for wasm.
  See [PR 4217].

[PR 4217]: https://github.com/libp2p/rust-libp2p/pull/4217

## 0.13.0

- Previously `libp2p-metrics::identify` would increase a counter / gauge / histogram on each
  received identify information. These metrics are misleading, as e.g. they depend on the identify
  interval and don't represent the set of currently connected peers. With this change, identify
  information is tracked for the currently connected peers only. Instead of an increase on each
  received identify information, metrics represent the status quo (Gauge).

  Metrics removed:
  - `libp2p_identify_protocols`
  - `libp2p_identify_received_info_listen_addrs`
  - `libp2p_identify_received_info_protocols`
  - `libp2p_identify_listen_addresses`

  Metrics added:
  - `libp2p_identify_remote_protocols`
  - `libp2p_identify_remote_listen_addresses`
  - `libp2p_identify_local_observed_addresses`

  See [PR 3325].

- Raise MSRV to 1.65.
  See [PR 3715].

- Replace `libp2p_swarm_connections_closed` `Counter` with `libp2p_swarm_connections_duration` `Histogram` which additionally tracks the duration of a connection.
  Note that you can use the `_count` metric of the `Histogram` as a replacement for the `Counter`.
  See [PR 3927].

- Remove the `pong_received` counter because it is no longer exposed by `libp2p-ping`.
  See [PR 3947].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3927]: https://github.com/libp2p/rust-libp2p/pull/3927
[PR 3325]: https://github.com/libp2p/rust-libp2p/pull/3325
[PR 3947]: https://github.com/libp2p/rust-libp2p/pull/3947

## 0.12.0

- Update to `prometheus-client` `v0.19.0`. See [PR 3207].

- Add `connections_establishment_duration` metric. See [PR 3134].

- Bump MSRV to 1.65.0.

- Update to `libp2p-core` `v0.39.0`.

- Update to `libp2p-dcutr` `v0.9.0`.

- Update to `libp2p-ping` `v0.42.0`.

- Update to `libp2p-kad` `v0.43.0`.

- Update to `libp2p-relay` `v0.15.0`.

- Update to `libp2p-identify` `v0.42.0`.

- Update to `libp2p-swarm` `v0.42.0`.

[PR 3134]: https://github.com/libp2p/rust-libp2p/pull/3134/
[PR 3207]: https://github.com/libp2p/rust-libp2p/pull/3207/

## 0.11.0

- Update to `libp2p-dcutr` `v0.8.0`.

- Update to `libp2p-identify` `v0.41.0`.

- Update to `libp2p-relay` `v0.14.0`.

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Update to `libp2p-ping` `v0.41.0`.

- Update to `libp2p-kad` `v0.42.0`.

- Update to `libp2p-gossipsub` `v0.43.0`.

- Add `protocol_stack` metrics. See [PR 2982].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

- Changed `Metrics::query_result_get_record_ok` from `Histogram` to a `Counter`.
  See [PR 2712].

[PR 2982]: https://github.com/libp2p/rust-libp2p/pull/2982/
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090
[PR 2712]: https://github.com/libp2p/rust-libp2p/pull/2712

## 0.10.0

- Update to `libp2p-swarm` `v0.40.0`.

- Update to `libp2p-dcutr` `v0.7.0`.

- Update to `libp2p-ping` `v0.40.0`.

- Update to `libp2p-identify` `v0.40.0`.

- Update to `libp2p-relay` `v0.13.0`.

- Update to `libp2p-kad` `v0.41.0`.

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-gossipsub` `v0.42.0`.

## 0.9.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-dcutr` `v0.6.0`.

- Update to `libp2p-ping` `v0.39.0`.

- Update to `libp2p-identify` `v0.39.0`.

- Update to `libp2p-relay` `v0.12.0`.

- Update to `libp2p-kad` `v0.40.0`.

- Update to `libp2p-core` `v0.36.0`.

## 0.8.0

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-dcutr` `v0.5.0`.

- Update to `libp2p-ping` `v0.38.0`.

- Update to `libp2p-identify` `v0.38.0`.

- Update to `libp2p-relay` `v0.11.0`.

- Update to `libp2p-kad` `v0.39.0`.

- Track number of connected nodes supporting a specific protocol via the identify protocol. See [PR 2734].

- Update to `libp2p-core` `v0.35.0`.

- Update to `prometheus-client` `v0.18.0`. See [PR 2822].

[PR 2822]: https://github.com/libp2p/rust-libp2p/pull/2761/

[PR 2734]: https://github.com/libp2p/rust-libp2p/pull/2734/

## 0.7.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

- Update to `libp2p-dcutr` `v0.4.0`.

- Update to `libp2p-ping` `v0.37.0`.

- Update to `libp2p-identify` `v0.37.0`.

- Update to `libp2p-relay` `v0.10.0`.

- Update to `libp2p-kad` `v0.38.0`.

## 0.6.1

- Update `dcutr` events from `libp2p_relay_events` to `libp2p_dcutr_events`, to avoid conflict with `relay` events.

## 0.6.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

- Update to `libp2p-dcutr` `v0.3.0`.

- Update to `libp2p-ping` `v0.36.0`.

- Update to `libp2p-identify` `v0.36.0`.

- Update to `libp2p-relay` `v0.9.0`.

- Update to `libp2p-kad` `v0.37.0`.

- Update to `prometheus-client` `v0.16.0`. See [PR 2631].

[PR 2631]: https://github.com/libp2p/rust-libp2p/pull/2631

## 0.5.0

- Update to `libp2p-swarm` `v0.35.0`.

- Update to `libp2p-dcutr` `v0.2.0`.

- Update to `libp2p-ping` `v0.35.0`.

- Update to `libp2p-identify` `v0.35.0`.

- Update to `libp2p-relay` `v0.8.0`.

- Update to `libp2p-kad` `v0.36.0`.

## 0.4.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Update to `libp2p-ping` `v0.34.0`.

- Update to `libp2p-identify` `v0.34.0`.

- Update to `libp2p-relay` `v0.7.0`.

- Update to `libp2p-kad` `v0.35.0`.

- Move from `open-metrics-client` to `prometheus-client` (see [PR 2442]).

- Drop support for gossipsub in wasm32-unknown-unknown target (see [PR 2506]).

[PR 2442]: https://github.com/libp2p/rust-libp2p/pull/2442

[PR 2506]: https://github.com/libp2p/rust-libp2p/pull/2506

## 0.3.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.2.0 [2021-11-16]

- Include gossipsub metrics (see [PR 2316]).

- Update dependencies.

[PR 2316]: https://github.com/libp2p/rust-libp2p/pull/2316

## 0.1.0 [2021-11-01]

- Add initial version.
