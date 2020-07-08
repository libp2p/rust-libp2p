# 0.20.1 [2020-07-08]

- Documentation updates.

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
