## 0.5.0

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

<!-- Update to libp2p-core v0.43.0 -->

## 0.4.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.3.1

- Add function to mutate `ConnectionLimits`.
  See [PR 4964](https://github.com/libp2p/rust-libp2p/pull/4964).

## 0.3.0


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
