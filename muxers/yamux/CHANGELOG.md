# 0.26.0 [unreleased]

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
