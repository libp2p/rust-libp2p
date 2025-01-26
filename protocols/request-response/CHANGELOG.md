## 0.28.0

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

- Add connection id to the events emitted by a request-response `Behaviour`.
  See [PR 5719](https://github.com/libp2p/rust-libp2p/pull/5719).

- Allow configurable request and response sizes for `json` and `cbor` codec.
  See [PR 5792](https://github.com/libp2p/rust-libp2p/pull/5792).

<!-- Update to libp2p-core v0.43.0 -->

## 0.27.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.26.4

- Use `web-time` instead of `instant`.
  See [PR 5347](https://github.com/libp2p/rust-libp2p/pull/5347).

## 0.26.3

- Report failure when streams are at capacity.
  See [PR 5417](https://github.com/libp2p/rust-libp2p/pull/5417).

- Report dial IO errors to the user.
  See [PR 5429](https://github.com/libp2p/rust-libp2p/pull/5429).

## 0.26.2

- Deprecate `Behaviour::add_address` in favor of `Swarm::add_peer_address`.
  See [PR 4371](https://github.com/libp2p/rust-libp2p/pull/4371).

## 0.26.1

- Derive `PartialOrd` and `Ord` for `{Out,In}boundRequestId`.
  See [PR 4956](https://github.com/libp2p/rust-libp2p/pull/4956).

## 0.26.0

- Remove `request_response::Config::set_connection_keep_alive` in favor of `SwarmBuilder::idle_connection_timeout`.
  See [PR 4679](https://github.com/libp2p/rust-libp2p/pull/4679).
- Allow at most 100 concurrent inbound + outbound streams per instance of `request_response::Behaviour`.
  This limit is configurable via `Config::with_max_concurrent_streams`.
  See [PR 3914](https://github.com/libp2p/rust-libp2p/pull/3914).
- Report IO failures on inbound and outbound streams.
  See [PR 3914](https://github.com/libp2p/rust-libp2p/pull/3914).
- Introduce dedicated types for `InboundRequestId` and `OutboundRequestId`.
  See [PR 3914](https://github.com/libp2p/rust-libp2p/pull/3914).
- Keep peer addresses in `HashSet` instead of `SmallVec` to prevent adding duplicate addresses.
  See [PR 4700](https://github.com/libp2p/rust-libp2p/pull/4700).

## 0.25.2

- Deprecate `request_response::Config::set_connection_keep_alive` in favor of `SwarmBuilder::idle_connection_timeout`.
  See [PR 4029](https://github.com/libp2p/rust-libp2p/pull/4029).

<!-- Internal changes

- Allow deprecated usage of `KeepAlive::Until`

-->

## 0.25.1

- Replace unmaintained `serde_cbor` dependency with `cbor4ii`.
  See [PR 4187].

[PR 4187]: https://github.com/libp2p/rust-libp2p/pull/4187

## 0.25.0

- Add `request_response::json::Behaviour` and `request_response::cbor::Behaviour` building on top of the `serde` traits.
  To conveniently construct these, we remove the `Codec` parameter from `Behaviour::new` and add `Behaviour::with_codec`.
  See [PR 3952].

- Raise MSRV to 1.65.
  See [PR 3715].
- Remove deprecated `RequestResponse` prefixed items. See [PR 3702].

- Remove `InboundFailure::UnsupportedProtocols` and `InboundFailure::InboundTimeout`.
  These variants are no longer constructed.
  See [PR 3605].

- Don't close connections if individual streams fail.
  Log the error instead.
  See [PR 3913].

[PR 3952]: https://github.com/libp2p/rust-libp2p/pull/3952
[PR 3605]: https://github.com/libp2p/rust-libp2p/pull/3605
[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3702]: https://github.com/libp2p/rust-libp2p/pull/3702
[PR 3913]: https://github.com/libp2p/rust-libp2p/pull/3913

## 0.24.1

- Deprecate `handler`, `codec` modules to make them private. See [PR 3847].

[PR 3847]: https://github.com/libp2p/rust-libp2p/pull/3847

## 0.24.0

- Update to `libp2p-core` `v0.39.0`.

- Rename types as per [discussion 2174].
  `RequestResponse` has been renamed to `Behaviour`.
  The `RequestResponse` prefix has been removed from various types like `RequestResponseEvent`.
  Users should prefer importing the request_response protocol as a module (`use libp2p::request_response;`),
  and refer to its types via `request_response::`. For example: `request_response::Behaviour` or `request_response::Event`.
  See [PR 3159].

- Update to `libp2p-swarm` `v0.42.0`.

[discussion 2174]: https://github.com/libp2p/rust-libp2p/discussions/2174
[PR 3159]: https://github.com/libp2p/rust-libp2p/pull/3159

## 0.23.0

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Replace `RequestResponse`'s `NetworkBehaviour` implemention `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Replace `RequestResponseHandler`'s `ConnectionHandler` implemention `inject_*` methods
  with the new `on_*` methods. See [PR 3085].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

[PR 3085]: https://github.com/libp2p/rust-libp2p/pull/3085
[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.22.0

- Bump rand to 0.8 and quickcheck to 1. See [PR 2857].

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

[PR 2857]: https://github.com/libp2p/rust-libp2p/pull/2857

## 0.21.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

## 0.20.0

- Update to `libp2p-swarm` `v0.38.0`.

- Update to `libp2p-core` `v0.35.0`.

## 0.19.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

## 0.18.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

## 0.17.0

- Update to `libp2p-swarm` `v0.35.0`.

## 0.16.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

## 0.15.0 [2022-01-27]

- Update dependencies.

- Remove unused `lru` crate (see [PR 2358]).

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2358]: https://github.com/libp2p/rust-libp2p/pull/2358

## 0.14.0 [2021-11-16]

- Use `instant` instead of `wasm-timer` (see [PR 2245]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245

## 0.13.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Manually implement `Debug` for `RequestResponseHandlerEvent` and
  `RequestProtocol`. See [PR 2183].

- Remove `RequestResponse::throttled` and the `throttled` module.
  See [PR 2236].

[PR 2183]: https://github.com/libp2p/rust-libp2p/pull/2183
[PR 2236]: https://github.com/libp2p/rust-libp2p/pull/2236

## 0.12.0 [2021-07-12]

- Update dependencies.

## 0.11.0 [2021-04-13]

- Update `libp2p-swarm`.
- Implement `std::error::Error` for `InboundFailure` and `OutboundFailure` [PR
  2033](https://github.com/libp2p/rust-libp2p/pull/2033).

## 0.10.0 [2021-03-17]

- Update `libp2p-swarm`.

- Close stream even when no response has been sent.
  [PR 1987](https://github.com/libp2p/rust-libp2p/pull/1987).

- Update dependencies.

## 0.9.1 [2021-02-15]

- Make `is_pending_outbound` return true on pending connection.
  [PR 1928](https://github.com/libp2p/rust-libp2p/pull/1928).

- Update dependencies.

## 0.9.0 [2021-01-12]

- Update dependencies.

- Re-export `throttled`-specific response channel. [PR
  1902](https://github.com/libp2p/rust-libp2p/pull/1902).

## 0.8.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

- Emit `InboundFailure::ConnectionClosed` for inbound requests that failed due
  to the underlying connection closing.
  [PR 1886](https://github.com/libp2p/rust-libp2p/pull/1886).

- Derive Clone for `InboundFailure` and `Outbound}Failure`.
  [PR 1891](https://github.com/libp2p/rust-libp2p/pull/1891)

## 0.7.0 [2020-12-08]

- Refine emitted events for inbound requests, introducing
  the `ResponseSent` event and the `ResponseOmission`
  inbound failures. This effectively removes previous
  support for one-way protocols without responses.
  [PR 1867](https://github.com/libp2p/rust-libp2p/pull/1867).

## 0.6.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

## 0.5.0 [2020-11-09]

- Update dependencies.

## 0.4.0 [2020-10-16]

- Update dependencies.

## 0.3.0 [2020-09-09]

- Add support for opt-in request-based flow-control to any
  request-response protocol via `RequestResponse::throttled()`.
  [PR 1726](https://github.com/libp2p/rust-libp2p/pull/1726).

- Update `libp2p-swarm` and `libp2p-core`.

## 0.2.0 [2020-08-18]

- Fixed connection keep-alive, permitting connections to close due
  to inactivity.
- Bump `libp2p-core` and `libp2p-swarm` dependencies.

## 0.1.1

- Always properly `close()` the substream after sending requests and
responses in the `InboundUpgrade` and `OutboundUpgrade`. Otherwise this is
left to `RequestResponseCodec::write_request` and `RequestResponseCodec::write_response`,
which can be a pitfall and lead to subtle problems (see e.g.
https://github.com/libp2p/rust-libp2p/pull/1606).

## 0.1.0

- Initial release.
