# 0.22.0 [unreleased]

- Bump `libp2p-core` dependency.

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
