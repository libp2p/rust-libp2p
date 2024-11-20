## 0.46.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.45.2

- Update `yamux` to version `v0.13.3`.`

## 0.45.1

- Deprecate `WindowUpdateMode::on_receive`.
  It does not enforce flow-control, i.e. breaks backpressure.
  Use `WindowUpdateMode::on_read` instead.
  See `yamux` crate version `v0.12.1` and [Yamux PR #177](https://github.com/libp2p/rust-yamux/pull/177).
- `yamux` `v0.13` enables auto-tuning for the Yamux stream receive window.
  While preserving small buffers on low-latency and/or low-bandwidth connections, this change allows for high-latency and/or high-bandwidth connections to exhaust the available bandwidth on a single stream.
  Have `libp2p-yamux` use `yamux` `v0.13` (new version) by default and fall back to `yamux` `v0.12` (old version) when setting any configuration options.
  Thus default users benefit from the increased performance, while power users with custom configurations maintain the old behavior.
  `libp2p-yamux` will switch over to `yamux` `v0.13` entirely with the next breaking release.
  See [PR 4970](https://github.com/libp2p/rust-libp2p/pull/4970).

## 0.45.0

- Migrate to `{In,Out}boundConnectionUpgrade` traits.
  See [PR 4695](https://github.com/libp2p/rust-libp2p/pull/4695).

## 0.44.1

- Update to `yamux` `v0.12` which brings performance improvements and introduces an ACK backlog of 256 inbound streams.
  When interacting with other libp2p nodes that are also running this or a newer version, the creation of inbound streams will be backpressured once the ACK backlog is hit.
  See [PR 3013].

[PR 3013]: https://github.com/libp2p/rust-libp2p/pull/3013

## 0.44.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Remove deprecated items.
  See [PR 3897].

- Remove `Incoming`, `LocalIncoming` and `LocalConfig` as well as anything from the underlying `yamux` crate from the public API.
  See [PR 3908].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3897]: https://github.com/libp2p/rust-libp2p/pull/3897
[PR 3908]: https://github.com/libp2p/rust-libp2p/pull/3908

## 0.43.1

- Drop `Yamux` prefix from all types.
  Users are encouraged to import the `yamux` module and refer to types via `yamux::Muxer`, `yamux::Config` etc.
  See [PR 3852].

[PR 3852]: https://github.com/libp2p/rust-libp2p/pull/3852

## 0.43.0

- Update to `libp2p-core` `v0.39.0`.

## 0.42.0

- Update to `libp2p-core` `v0.38.0`.

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.41.1

- Yield from `StreamMuxer::poll` as soon as we receive a single substream.
  This fixes [issue 3041].
  See [PR 3071].

[PR 3071]: https://github.com/libp2p/rust-libp2p/pull/3071/
[issue 3041]: https://github.com/libp2p/rust-libp2p/issues/3041/

## 0.41.0

- Update to `libp2p-core` `v0.37.0`.

## 0.40.0

- Update to `libp2p-core` `v0.36.0`

- Remove `OpenSubstreamToken` as it is dead code. See [PR 2873].

- Drive connection also via `StreamMuxer::poll`. Any received streams will be buffered up to a maximum of 25 streams.
  See [PR 2861].

[PR 2873]: https://github.com/libp2p/rust-libp2p/pull/2873/
[PR 2861]: https://github.com/libp2p/rust-libp2p/pull/2861/

## 0.39.0

- Update to `libp2p-core` `v0.35.0`

## 0.38.0

- Update to `libp2p-core` `v0.34.0`.

## 0.37.0

- Update to `libp2p-core` `v0.33.0`.

## 0.36.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `parking_lot` `v0.12.0`. See [PR 2463].

[PR 2463]: https://github.com/libp2p/rust-libp2p/pull/2463/

## 0.35.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- Update to `yamux` `v0.10.0` and thus defaulting to `WindowUpdateMode::OnRead`.
  See [PR 2435].

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2435]: https://github.com/libp2p/rust-libp2p/pull/2435

## 0.34.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Implement `From<io::Error> for YamuxError` instead of `Into`.
  [PR 2169](https://github.com/libp2p/rust-libp2p/pull/2169)

## 0.33.0 [2021-07-12]

- Update dependencies.

## 0.32.0 [2021-04-13]

- Update to `yamux` `v0.9.0` [PR
  1960](https://github.com/libp2p/rust-libp2p/pull/1960).

## 0.31.0 [2021-03-17]

- Update `libp2p-core`.

## 0.30.1 [2021-02-17]

- Update `yamux` to `0.8.1`.

## 0.30.0 [2021-01-12]

- Update dependencies.

## 0.29.0 [2020-12-17]

- Update `libp2p-core`.

## 0.28.0 [2020-11-25]

- Update `libp2p-core`.

## 0.27.0 [2020-11-09]

- Tweak the naming in the `MplexConfig` API for better
  consistency with `libp2p-mplex`.
  [PR 1822](https://github.com/libp2p/rust-libp2p/pull/1822).

- Update dependencies.

## 0.26.0 [2020-10-16]

- Update `libp2p-core`.

## 0.25.0 [2020-09-09]

- Update to `yamux-0.8.0`. Upgrade step 4 of 4. This version always implements
  the additive meaning w.r.t. initial window updates. The configuration option
  `lazy_open` is removed. Yamux will automatically send an initial window update
  if the receive window is configured to be larger than the default.

## 0.24.0 [2020-09-09]

- Update to `yamux-0.7.0`. Upgrade step 3 of 4. This version sets the flag but will
  always interpret initial window updates as additive.

## 0.23.0 [2020-09-09]

- Update to `yamux-0.6.0`. As explain below, this is step 2 of 4 in a multi-release
  upgrade. This version recognises and sets the flag that causes the new semantics
  for the initial window update.

## 0.22.0 [2020-09-09]

- Update to `yamux-0.5.0`. *This is the start of a multi-release transition* to a
  different behaviour w.r.t. the initial window update frame. Tracked in [[1]],
  this will change the initial window update to mean "in addition to the default
  receive window" rather than "the total receive window". This version recognises
  a temporary flag, that will cause the new meaning. The next version `0.23.0`
  will also set the flag. Version `0.24.0` will still set the flag but will always
  implement the new meaning, and (finally) version `0.25.0` will no longer set the
  flag and always use the additive semantics. If current code uses the default
  Yamux configuration, all upgrades need to be performed in order because each
  version is only compatible with its immediate predecessor. Alternatively, if the
  default configuration is deployed with `lazy_open` set to `true`, one can
  directly upgrade from version `0.21.0` to `0.25.0` without any intermediate
  upgrades.

- Bump `libp2p-core` dependency.

[1]: https://github.com/paritytech/yamux/issues/92

## 0.21.0 [2020-08-18]

- Bump `libp2p-core` dependency.

- Allow overriding the mode (client/server), e.g. in the context
of TCP hole punching. [PR 1691](https://github.com/libp2p/rust-libp2p/pull/1691).

## 0.20.0 [2020-07-01]

- Update `libp2p-core`, i.e. `StreamMuxer::poll_inbound` has been renamed
  to `poll_event` and returns a `StreamMuxerEvent`.

## 0.19.1 [2020-06-22]

- Deprecated method `Yamux::is_remote_acknowledged` has been removed
  as part of [PR 1616](https://github.com/libp2p/rust-libp2p/pull/1616).
