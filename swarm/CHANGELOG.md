# 0.21.0 [unreleased]

- The `cause` of `SwarmEvent::ConnectionClosed` is now an `Option`,
and `None` indicates an active connection close not caused by an
error.

- `DialError::Banned` has been added and is returned from `Swarm::dial`
if the peer is banned, thereby also invoking the `NetworkBehaviour::inject_dial_failure`
callback.

- Update the `libp2p-core` dependency to `0.21`, fixing [1584](https://github.com/libp2p/rust-libp2p/issues/1584).

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
