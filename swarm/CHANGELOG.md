# 0.23.0 [unreleased]

- Require a `Boxed` transport to be given to the `Swarm`
  or `SwarmBuilder` to avoid unnecessary double-boxing of
  transports and simplify API bounds.
  [PR 1794](https://github.com/libp2p/rust-libp2p/pull/1794)

- Respect inbound timeouts and upgrade versions in the `MultiHandler`.
  [PR 1786](https://github.com/libp2p/rust-libp2p/pull/1786).

- Instead of iterating each inbound and outbound substream upgrade looking for
  one to make progress, use a `FuturesUnordered` for both pending inbound and
  pending outbound upgrades. As a result only those upgrades are polled that are
  ready to progress.

  Implementors of `InboundUpgrade` and `OutboundUpgrade` need to ensure to wake
  up the underlying task once they are ready to make progress as they won't be
  polled otherwise.

  [PR 1775](https://github.com/libp2p/rust-libp2p/pull/1775)

# 0.22.0 [2020-09-09]

- Bump `libp2p-core` dependency.

- Adds `ProtocolsHandler::InboundOpenInfo` type which mirrors the existing
  `OutboundOpenInfo` type. A value of this type is passed as an extra argument
  to `ProtocolsHandler::inject_fully_negotiated_inbound` and
  `ProtocolsHandler::inject_listen_upgrade_error`.

- `SubstreamProtocol` now has a second type parameter corresponding to
  inbound or outbound information, a value of which is part of `SubstreamProtocol`
  now. Consequently `ProtocolsHandlerEvent::OutboundSubstreamRequest` no longer
  has a separate `info` field.

# 0.21.0 [2020-08-18]

- Add missing delegation calls in some `ProtocolsHandler` wrappers.
See [PR 1710](https://github.com/libp2p/rust-libp2p/pull/1710).

- Add as_ref and as_mut functions to Toggle
[PR 1684](https://github.com/libp2p/rust-libp2p/pull/1684).

- The `cause` of `SwarmEvent::ConnectionClosed` is now an `Option`,
and `None` indicates an active connection close not caused by an
error.

- `DialError::Banned` has been added and is returned from `Swarm::dial`
if the peer is banned, thereby also invoking the `NetworkBehaviour::inject_dial_failure`
callback.

- Update the `libp2p-core` dependency to `0.21`, fixing [1584](https://github.com/libp2p/rust-libp2p/issues/1584).

- Fix connections being kept alive by `OneShotHandler` when not handling any
  requests [PR 1698](https://github.com/libp2p/rust-libp2p/pull/1698).

# 0.20.1 [2020-07-08]

- Documentation updates.

- Ignore addresses returned by `NetworkBehaviour::addresses_of_peer`
that the `Swarm` considers to be listening addresses of the local node. This
avoids futile dialing attempts of a node to itself, which can otherwise
even happen in genuine situations, e.g. after the local node changed
its network identity and a behaviour makes a dialing attempt to a
former identity using the same addresses.

# 0.20.0 [2020-07-01]

- Updated the `libp2p-core` dependency.

- Add `ProtocolsHandler::inject_listen_upgrade_error`, the inbound
analogue of `ProtocolsHandler::inject_dial_upgrade_error`, with an
empty default implementation. No implementation is required to
retain existing behaviour.

- Add `ProtocolsHandler::inject_address_change` and
`NetworkBehaviour::inject_address_change` to notify of a change in
the address of an existing connection.

# 0.19.1 [2020-06-18]

- Bugfix: Fix MultiHandler panicking when empty
  ([PR 1598](https://github.com/libp2p/rust-libp2p/pull/1598)).
