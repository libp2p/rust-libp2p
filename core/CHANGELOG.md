# 0.37.0

- Implement `Hash` and `Ord` for `PublicKey`. See [PR 2915].

- Remove default features. If you previously depended on `secp256k1` or `ecdsa` you need to enable these explicitly
  now. See [PR 2918].

- Deprecate `StreamMuxerExt::next_{inbound,outbound}`. See [PR 3002].

[PR 2915]: https://github.com/libp2p/rust-libp2p/pull/2915
[PR 2918]: https://github.com/libp2p/rust-libp2p/pull/2918
[PR 3002]: https://github.com/libp2p/rust-libp2p/pull/3002

# 0.36.0

- Make RSA keypair support optional. To enable RSA support, `rsa` feature should be enabled.
  See [PR 2860].

- Add `ReadyUpgrade`. See [PR 2855].

[PR 2855]: https://github.com/libp2p/rust-libp2p/pull/2855
[PR 2860]: https://github.com/libp2p/rust-libp2p/pull/2860/

# 0.35.1

- Update to `p256` `v0.11.0`. See [PR 2636].

[PR 2636]: https://github.com/libp2p/rust-libp2p/pull/2636/

# 0.35.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].
- Drop `Unpin` requirement from `SubstreamBox`. See [PR 2762] and [PR 2776].
- Drop `Sync` requirement on `StreamMuxer` for constructing `StreamMuxerBox`. See [PR 2775].
- Use `Pin<&mut Self>` as the receiver type for all `StreamMuxer` poll functions. See [PR 2765].
- Change `StreamMuxer` interface to be entirely poll-based. All functions on `StreamMuxer` now
  require a `Context` and return `Poll`. This gives callers fine-grained control over what they
  would like to make progress on. See [PR 2724] and [PR 2797].

[PR 2724]: https://github.com/libp2p/rust-libp2p/pull/2724
[PR 2762]: https://github.com/libp2p/rust-libp2p/pull/2762
[PR 2775]: https://github.com/libp2p/rust-libp2p/pull/2775
[PR 2776]: https://github.com/libp2p/rust-libp2p/pull/2776
[PR 2765]: https://github.com/libp2p/rust-libp2p/pull/2765
[PR 2797]: https://github.com/libp2p/rust-libp2p/pull/2797
[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

# 0.34.0

- Remove `{read,write,flush,shutdown,destroy}_substream` functions from `StreamMuxer` trait
  in favor of forcing `StreamMuxer::Substream` to implement `AsyncRead + AsyncWrite`. See [PR 2707].
- Replace `Into<std::io::Error>` bound on `StreamMuxer::Error` with `std::error::Error`. See [PR 2710].

- Remove the concept of individual `Transport::Listener` streams from `Transport`.
  Instead the `Transport` is polled directly via `Transport::poll`. The
  `Transport` is now responsible for driving its listeners. See [PR 2652].

[PR 2691]: https://github.com/libp2p/rust-libp2p/pull/2691
[PR 2707]: https://github.com/libp2p/rust-libp2p/pull/2707
[PR 2710]: https://github.com/libp2p/rust-libp2p/pull/2710
[PR 2652]: https://github.com/libp2p/rust-libp2p/pull/2652

# 0.33.0

- Have methods on `Transport` take `&mut self` instead of `self`. See [PR 2529].
- Remove `StreamMuxer::flush_all`. See [PR 2669].
- Rename `StreamMuxer::close` to `StreamMuxer::poll_close`. See [PR 2666].
- Remove deprecated function `StreamMuxer::is_remote_acknowledged`. See [PR 2665].

[PR 2529]: https://github.com/libp2p/rust-libp2p/pull/2529
[PR 2666]: https://github.com/libp2p/rust-libp2p/pull/2666
[PR 2665]: https://github.com/libp2p/rust-libp2p/pull/2665
[PR 2669]: https://github.com/libp2p/rust-libp2p/pull/2669


# 0.32.1

- Add `PeerId::try_from_multiaddr` to extract a `PeerId` from a `Multiaddr` that ends in `/p2p/<peer-id>`.

# 0.32.0 [2022-02-22]

- Remove `Network`. `libp2p-core` is from now on an auxiliary crate only. Users
  that have previously used `Network` only, will need to use `Swarm` instead. See
  [PR 2492].

- Update to `multiaddr` `v0.14.0`.

- Update to `multihash` `v0.16.0`.

- Implement `Display` on `DialError`. See [PR 2456].

- Update to `parking_lot` `v0.12.0`. See [PR 2463].

- Validate PeerRecord signature matching peer ID. See [RUSTSEC-2022-0009].

- Don't take ownership of key in `PeerRecord::new` and `SignedEnvelope::new`. See [PR 2516].

- Remove `SignedEnvelope::payload` in favor of
  `SignedEnvelope::payload_and_signing_key`. The caller is expected to check
  that the returned signing key makes sense in the payload's context. See [PR 2522].

[PR 2456]: https://github.com/libp2p/rust-libp2p/pull/2456
[RUSTSEC-2022-0009]: https://rustsec.org/advisories/RUSTSEC-2022-0009.html
[PR 2492]: https://github.com/libp2p/rust-libp2p/pull/2492
[PR 2516]: https://github.com/libp2p/rust-libp2p/pull/2516
[PR 2463]: https://github.com/libp2p/rust-libp2p/pull/2463/
[PR 2522]: https://github.com/libp2p/rust-libp2p/pull/2522

# 0.31.0 [2022-01-27]

- Update dependencies.

- Report concrete connection IDs in `NetworkEvent::ConnectionEstablished` and
  `NetworkEvent::ConnectionClosed` (see [PR 2350]).

- Migrate to Rust edition 2021 (see [PR 2339]).

- Add support for ECDSA identities (see [PR 2352]).

- Add `ConnectedPoint::is_relayed` (see [PR 2392]).

- Enable overriding _dial concurrency factor_ per dial via
  `DialOpts::override_dial_concurrency_factor`.

  - Introduces `libp2p_core::DialOpts` mirroring `libp2p_swarm::DialOpts`.
      Passed as an argument to `Network::dial`.
  - Removes `Peer::dial` in favor of `Network::dial`.

  See [PR 2404].

- Implement `Serialize` and `Deserialize` for `PeerId` (see [PR 2408])

- Report negotiated and expected `PeerId` as well as remote address in
  `DialError::WrongPeerId` (see [PR 2428]).

- Allow overriding role when dialing. This option is needed for NAT and firewall
  hole punching.

    - Add `Transport::dial_as_listener`. As `Transport::dial` but
      overrides the role of the local node on the connection . I.e. has the
      local node act as a listener on the outgoing connection.

    - Add `override_role` option to `DialOpts`.

  See [PR 2363].

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2350]: https://github.com/libp2p/rust-libp2p/pull/2350
[PR 2352]: https://github.com/libp2p/rust-libp2p/pull/2352
[PR 2392]: https://github.com/libp2p/rust-libp2p/pull/2392
[PR 2404]: https://github.com/libp2p/rust-libp2p/pull/2404
[PR 2408]: https://github.com/libp2p/rust-libp2p/pull/2408
[PR 2428]: https://github.com/libp2p/rust-libp2p/pull/2428
[PR 2363]: https://github.com/libp2p/rust-libp2p/pull/2363

# 0.30.1 [2021-11-16]

- Use `instant` instead of `wasm-timer` (see [PR 2245]).

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245

# 0.30.0 [2021-11-01]

- Add `ConnectionLimit::with_max_established` (see [PR 2137]).

- Add `Keypair::to_protobuf_encoding` (see [PR 2142]).

- Change `PublicKey::into_protobuf_encoding` to `PublicKey::to_protobuf_encoding` (see [PR 2145]).

- Change `PublicKey::into_peer_id` to `PublicKey::to_peer_id` (see [PR 2145]).

- Change `PeerId::from_public_key(PublicKey)` to `PeerId::from_public_key(&PublicKey)` (see [PR 2145]).

- Add `From<&PublicKey> for PeerId` (see [PR 2145]).

- Remove `TInEvent` and `TOutEvent` trait paramters on most public types.
  `TInEvent` and `TOutEvent` are implied through `THandler` and thus
  superflucious. Both are removed in favor of a derivation through `THandler`
  (see [PR 2183]).

- Require `ConnectionHandler::{InEvent,OutEvent,Error}` to implement `Debug`
  (see [PR 2183]).

- Remove `DisconnectedPeer::set_connected` and `Pool::add` (see [PR 2195]).

- Report `ConnectionLimit` error through `ConnectionError` and thus through
  `NetworkEvent::ConnectionClosed` instead of previously through
  `PendingConnectionError` and thus `NetworkEvent::{IncomingConnectionError,
  DialError}` (see [PR 2191]).

- Report abortion of pending connection through `DialError`,
  `UnknownPeerDialError` or `IncomingConnectionError` (see [PR 2191]).

- Remove deprecated functions `upgrade::write_one`, `upgrade::write_with_len_prefix`
  and `upgrade::read_one` (see [PR 2213]).

- Add `SignedEnvelope` and `PeerRecord` according to [RFC0002] and [RFC0003]
  (see [PR 2107]).

- Report `ListenersEvent::Closed` when dropping a listener in `ListenersStream::remove_listener`,
  return `bool` instead of `Result<(), ()>` (see [PR 2261]).

- Concurrently dial address candidates within a single dial attempt (see [PR 2248]) configured
  via `Network::with_dial_concurrency_factor`.

  - On success of a single address, provide errors of the thus far failed dials via
    `NetworkEvent::ConnectionEstablished::outgoing`.

  - On failure of all addresses, provide the errors via `NetworkEvent::DialError`.

[PR 2145]: https://github.com/libp2p/rust-libp2p/pull/2145
[PR 2213]: https://github.com/libp2p/rust-libp2p/pull/2213
[PR 2142]: https://github.com/libp2p/rust-libp2p/pull/2142
[PR 2137]: https://github.com/libp2p/rust-libp2p/pull/2137
[PR 2183]: https://github.com/libp2p/rust-libp2p/pull/2183
[PR 2191]: https://github.com/libp2p/rust-libp2p/pull/2191
[PR 2195]: https://github.com/libp2p/rust-libp2p/pull/2195
[PR 2107]: https://github.com/libp2p/rust-libp2p/pull/2107
[PR 2248]: https://github.com/libp2p/rust-libp2p/pull/2248
[PR 2261]: https://github.com/libp2p/rust-libp2p/pull/2261
[RFC0002]: https://github.com/libp2p/specs/blob/master/RFC/0002-signed-envelopes.md
[RFC0003]: https://github.com/libp2p/specs/blob/master/RFC/0003-routing-records.md

# 0.29.0 [2021-07-12]

- Switch from `parity-multiaddr` to upstream `multiaddr`.

- Update dependencies.

- Implement `Keypair::from_protobuf_encoding` for ed25519 keys (see [PR 2090]).

- Deprecate `upgrade::write_one`.
  Deprecate `upgrade::write_with_len_prefix`.
  Deprecate `upgrade::read_one`.
  Introduce `upgrade::read_length_prefixed` and `upgrade::write_length_prefixed`.
  See [PR 2111](https://github.com/libp2p/rust-libp2p/pull/2111).

[PR 2090]: https://github.com/libp2p/rust-libp2p/pull/2090

# 0.28.3 [2021-04-26]

- Fix build with secp256k1 disabled [PR 2057](https://github.com/libp2p/rust-libp2p/pull/2057).

# 0.28.2 [2021-04-13]

- Update dependencies.

# 0.28.1 [2021-03-17]

- Update `paritytech-multiaddr` to `>=v0.11.2`.

# 0.28.0 [2021-03-17]

- `Network::dial()` understands `/p2p` addresses and `Transport::dial`
  gets a "fully qualified" `/p2p` address when dialing a specific peer,
  whether through the `Network::peer()` API or via `Network::dial()`
  with a `/p2p` address.

- `Network::dial()` and `network::Peer::dial()` return a `DialError`
  on error.

- Shorten and unify `Debug` impls of public keys.

# 0.27.1 [2021-02-15]

- Update dependencies.

# 0.27.0 [2021-01-12]

- (Re)add `Transport::address_translation` to permit transport-specific
  translations of observed addresses onto listening addresses.
  [PR 1887](https://github.com/libp2p/rust-libp2p/pull/1887)

- Update dependencies.

# 0.26.0 [2020-12-17]

- Make `PeerId` be `Copy`, including small `PeerId` API changes.
  [PR 1874](https://github.com/libp2p/rust-libp2p/pull/1874/).

# 0.25.2 [2020-12-02]

- Require `multistream-select-0.9.1`.

# 0.25.1 [2020-11-25]

- Add missing multiaddr upgrade.

# 0.25.0 [2020-11-25]

- The `NetworkConfig` API is now a builder that moves `self`.
  [PR 1848](https://github.com/libp2p/rust-libp2p/pull/1848/).

- New configurable connection limits for established connections and
  dedicated connection counters. Removed the connection limit dedicated
  to outgoing pending connection _per peer_. Connection limits are now
  represented by `u32` intead of `usize` types.
  [PR 1848](https://github.com/libp2p/rust-libp2p/pull/1848/).

- Update `multihash`.

- Update `multistream-select`.

# 0.24.0 [2020-11-09]

- Remove `ConnectionInfo` trait and replace it with `PeerId`
  everywhere. This was already effectively the case because
  `ConnectionInfo` was implemented on `PeerId`.

# 0.23.1 [2020-10-20]

- Update dependencies.

# 0.23.0 [2020-10-16]

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
