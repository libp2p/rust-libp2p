# 0.21.0 [2020-08-18]

- Add `transport::Builder::multiplex_ext` for further customisation during
`StreamMuxer` creation. [PR 1691](https://github.com/libp2p/rust-libp2p/pull/1691).

- Refactoring of connection close and disconnect behaviour.  In particular, the former
  `NetworkEvent::ConnectionError` is now `NetworkEvent::ConnectionClosed` with the `error`
  field being an `Option` and `None` indicating an active (but not necessarily orderly) close.
  This guarantees that `ConnectionEstablished` events are always eventually paired
  with `ConnectionClosed` events, regardless of how connections are closed.
  Correspondingly, `EstablishedConnection::close` is now `EstablishedConnection::start_close`
  to reflect that an orderly close completes asynchronously in the background, with the
  outcome observed by continued polling of the `Network`. In contrast, `disconnect`ing
  a peer takes effect immediately without an orderly connection shutdown.
  See [PR 1619](https://github.com/libp2p/rust-libp2p/pull/1619) for further details.

- Add `ConnectedPoint::get_remote_address`
  ([PR 1649](https://github.com/libp2p/rust-libp2p/pull/1649)).

# 0.20.1 [2020-07-17]

- Update ed25519-dalek dependency.

# 0.20.0 [2020-07-01]

- Conditional compilation fixes for the `wasm32-wasi` target
  ([PR 1633](https://github.com/libp2p/rust-libp2p/pull/1633)).

- Rename `StreamMuxer::poll_inbound` to `poll_event` and change the
return value to `StreamMuxerEvent`. This new `StreamMuxerEvent` makes
it possible for the multiplexing layer to notify the upper layers of
a change in the address of the underlying connection.

- Add `ConnectionHandler::inject_address_change`.

# 0.19.2 [2020-06-22]

- Add PartialOrd and Ord for PeerId
  ([PR 1594](https://github.com/libp2p/rust-libp2p/pull/1594)).

- Updated dependencies.

- Deprecate `StreamMuxer::is_remote_acknowledged`
  ([PR 1616](https://github.com/libp2p/rust-libp2p/pull/1616)).
