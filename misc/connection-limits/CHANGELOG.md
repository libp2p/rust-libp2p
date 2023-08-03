## 0.2.1

- Do not count a connection as established when it is denied by another sibling `NetworkBehaviour`.
  In other words, do not increase established connection counter in `handle_established_outbound_connection` or `handle_established_inbound_connection`, but in `FromSwarm::ConnectionEstablished` instead.

  See [PR 4250].

- Decrease `pending_inbound_connections` on `FromSwarm::ListenFailure` and `pending_outbound_connections` on `FromSwarm::DialFailure`.

  See [PR 4250].

[PR 4250]: https://github.com/libp2p/rust-libp2p/pull/4250

## 0.2.0


- Raise MSRV to 1.65.
  See [PR 3715].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715

## 0.1.0

- Initial release.
