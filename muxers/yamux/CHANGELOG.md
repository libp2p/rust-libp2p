# 0.40.0

- Update to `libp2p-core` `v0.36.0`

- Remove `OpenSubstreamToken` as it is dead code. See [PR 2873].

[PR 2873]: https://github.com/libp2p/rust-libp2p/pull/2873/

# 0.39.0

- Update to `libp2p-core` `v0.35.0`

# 0.38.0

- Update to `libp2p-core` `v0.34.0`.

# 0.37.0

- Update to `libp2p-core` `v0.33.0`.

# 0.36.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `parking_lot` `v0.12.0`. See [PR 2463].

[PR 2463]: https://github.com/libp2p/rust-libp2p/pull/2463/

# 0.35.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

- Update to `yamux` `v0.10.0` and thus defaulting to `WindowUpdateMode::OnRead`.
  See [PR 2435].

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2435]: https://github.com/libp2p/rust-libp2p/pull/2435

# 0.34.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Implement `From<io::Error> for YamuxError` instead of `Into`.
  [PR 2169](https://github.com/libp2p/rust-libp2p/pull/2169)

# 0.33.0 [2021-07-12]

- Update dependencies.

# 0.32.0 [2021-04-13]

- Update to `yamux` `v0.9.0` [PR
  1960](https://github.com/libp2p/rust-libp2p/pull/1960).

# 0.31.0 [2021-03-17]

- Update `libp2p-core`.

# 0.30.1 [2021-02-17]

- Update `yamux` to `0.8.1`.

# 0.30.0 [2021-01-12]

- Update dependencies.

# 0.29.0 [2020-12-17]

- Update `libp2p-core`.

# 0.28.0 [2020-11-25]

- Update `libp2p-core`.

# 0.27.0 [2020-11-09]

- Tweak the naming in the `MplexConfig` API for better
  consistency with `libp2p-mplex`.
  [PR 1822](https://github.com/libp2p/rust-libp2p/pull/1822).

- Update dependencies.

# 0.26.0 [2020-10-16]

- Update `libp2p-core`.

# 0.25.0 [2020-09-09]

- Update to `yamux-0.8.0`. Upgrade step 4 of 4. This version always implements
  the additive meaning w.r.t. initial window updates. The configuration option
  `lazy_open` is removed. Yamux will automatically send an initial window update
  if the receive window is configured to be larger than the default.

# 0.24.0 [2020-09-09]

- Update to `yamux-0.7.0`. Upgrade step 3 of 4. This version sets the flag but will
  always interpret initial window updates as additive.

# 0.23.0 [2020-09-09]

- Update to `yamux-0.6.0`. As explain below, this is step 2 of 4 in a multi-release
  upgrade. This version recognises and sets the flag that causes the new semantics
  for the initial window update.

# 0.22.0 [2020-09-09]

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

# 0.21.0 [2020-08-18]

- Bump `libp2p-core` dependency.

- Allow overriding the mode (client/server), e.g. in the context
of TCP hole punching. [PR 1691](https://github.com/libp2p/rust-libp2p/pull/1691).

# 0.20.0 [2020-07-01]

- Update `libp2p-core`, i.e. `StreamMuxer::poll_inbound` has been renamed
  to `poll_event` and returns a `StreamMuxerEvent`.

# 0.19.1 [2020-06-22]

- Deprecated method `Yamux::is_remote_acknowledged` has been removed
  as part of [PR 1616](https://github.com/libp2p/rust-libp2p/pull/1616).
