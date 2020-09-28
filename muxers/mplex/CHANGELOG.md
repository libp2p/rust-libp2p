# 0.23.0 [unreleased]

- Address a potential stall when reading from substreams.

- Send a `Reset` or `Close` to the remote when a substream is dropped,
  as appropriate for the current state of the substream,
  removing that substream from the tracked open substreams,
  to avoid artificially running into substream limits.

- Change the semantics of the `max_substreams` configuration. Now,
  outbound substream attempts beyond the configured limit are delayed,
  with a task wakeup once an existing substream closes, i.e. the limit
  results in back-pressure for new outbound substreams. New inbound
  substreams beyond the limit are immediately answered with a `Reset`.
  If too many (by some internal threshold) pending frames accumulate,
  e.g. as a result of an aggressive number of inbound substreams being
  opened beyond the configured limit, the connection is closed ("DoS protection").

- Update dependencies.

# 0.22.0 [2020-09-09]

- Bump `libp2p-core` dependency.

# 0.21.0 [2020-08-18]

- Bump `libp2p-core` dependency.

# 0.20.0 [2020-07-01]

- Update `libp2p-core`, i.e. `StreamMuxer::poll_inbound` has been renamed
  to `poll_event` and returns a `StreamMuxerEvent`.

# 0.19.2 [2020-06-22]

- Deprecated method `Multiplex::is_remote_acknowledged` has been removed
  as part of [PR 1616](https://github.com/libp2p/rust-libp2p/pull/1616).

- Updated dependencies.
