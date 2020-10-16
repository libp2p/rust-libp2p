# 0.23.0 [unreleased]

- Rework transport boxing and move timeout configuration
  to the transport builder.
  [PR 1794](https://github.com/libp2p/rust-libp2p/pull/1794).

- Update dependencies.

# 0.22.1 [2020-09-10]

- Require at least parity-multiaddr v0.9.2 in order to fulfill `Ord` bound on
  `Multiaddr`. [PR 1742](https://github.com/libp2p/rust-libp2p/pull/1742).

# 0.22.0 [2020-09-09]

- Simplify incoming connection handling. The `IncomingConnectionEvent`
  has been removed. Instead, pass the `IncomingConnection` obtained
  from `NetworkEvent::IncomingConnection` to `Network::accept()`.
  [PR 1732](https://github.com/libp2p/rust-libp2p/pull/1732).

- Allow any closure to be passed as an executor.
  [PR 1686](https://github.com/libp2p/rust-libp2p/pull/1686)

- Remove `PeerId` compatibility mode for "identity" and SHA2 hashes.
  Historically, before 0.12, `PeerId`s were incorrectly always hashed with SHA2.
  Starting from version 0.13, rust-libp2p accepted both hashed and non-hashed keys as
  input.  Starting from version 0.16 rust-libp2p compared `PeerId`s of "identity" and
  SHA2 hashes equal, which made it possible to connect through secio or noise to nodes
  with an identity hash for the same peer ID. Starting from version 0.17, rust-libp2p
  switched to not hashing the key (i.e. the correct behaviour) while retaining
  equality between peer IDs using the "identity" hash and SHA2. Finally, with
  this release, that will no longer be the case and it is assumed that peer IDs
  whose length is less or equal to 42 bytes always use the "identity" hash so
  two peer IDs are equal if and only if they use the same hash algorithm and
  have the same hash digest. [PR 1608](https://github.com/libp2p/rust-libp2p/pull/1608).

- Return dialer address instead of listener address as `remote_addr` in
  `MemoryTransport` `Listener` `ListenerEvent::Upgrade`
  [PR 1724](https://github.com/libp2p/rust-libp2p/pull/1724).

# 0.21.0 [2020-08-18]

- Remove duplicates when performing address translation
  [PR 1697](https://github.com/libp2p/rust-libp2p/pull/1697).

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
