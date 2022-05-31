# 0.18.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

# 0.17.0

- Update to `libp2p-swarm` `v0.35.0`.

# 0.16.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see PR 2445).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

# 0.15.0 [2022-01-27]

- Update dependencies.

- Remove unused `lru` crate (see [PR 2358]).

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2358]: https://github.com/libp2p/rust-libp2p/pull/2358

# 0.14.0 [2021-11-16]

- Use `instant` instead of `wasm-timer` (see [PR 2245]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245

# 0.13.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Manually implement `Debug` for `RequestResponseHandlerEvent` and
  `RequestProtocol`. See [PR 2183].

- Remove `RequestResponse::throttled` and the `throttled` module.
  See [PR 2236].

[PR 2183]: https://github.com/libp2p/rust-libp2p/pull/2183
[PR 2236]: https://github.com/libp2p/rust-libp2p/pull/2236

# 0.12.0 [2021-07-12]

- Update dependencies.

# 0.11.0 [2021-04-13]

- Update `libp2p-swarm`.
- Implement `std::error::Error` for `InboundFailure` and `OutboundFailure` [PR
  2033](https://github.com/libp2p/rust-libp2p/pull/2033).

# 0.10.0 [2021-03-17]

- Update `libp2p-swarm`.

- Close stream even when no response has been sent.
  [PR 1987](https://github.com/libp2p/rust-libp2p/pull/1987).

- Update dependencies.

# 0.9.1 [2021-02-15]

- Make `is_pending_outbound` return true on pending connection.
  [PR 1928](https://github.com/libp2p/rust-libp2p/pull/1928).

- Update dependencies.

# 0.9.0 [2021-01-12]

- Update dependencies.

- Re-export `throttled`-specific response channel. [PR
  1902](https://github.com/libp2p/rust-libp2p/pull/1902).

# 0.8.0 [2020-12-17]

- Update `libp2p-swarm` and `libp2p-core`.

- Emit `InboundFailure::ConnectionClosed` for inbound requests that failed due
  to the underlying connection closing.
  [PR 1886](https://github.com/libp2p/rust-libp2p/pull/1886).

- Derive Clone for `InboundFailure` and `Outbound}Failure`.
  [PR 1891](https://github.com/libp2p/rust-libp2p/pull/1891)

# 0.7.0 [2020-12-08]

- Refine emitted events for inbound requests, introducing
  the `ResponseSent` event and the `ResponseOmission`
  inbound failures. This effectively removes previous
  support for one-way protocols without responses.
  [PR 1867](https://github.com/libp2p/rust-libp2p/pull/1867).

# 0.6.0 [2020-11-25]

- Update `libp2p-swarm` and `libp2p-core`.

# 0.5.0 [2020-11-09]

- Update dependencies.

# 0.4.0 [2020-10-16]

- Update dependencies.

# 0.3.0 [2020-09-09]

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
