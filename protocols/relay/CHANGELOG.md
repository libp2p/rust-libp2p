## 0.18.1

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

## 0.18.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.17.3
- Use `web-time` instead of `instant`.
  See [PR 5347](https://github.com/libp2p/rust-libp2p/pull/5347).
- Fix manual closure of relayed listener.
  See [PR 5491](https://github.com/libp2p/rust-libp2p/pull/5491)
- Add resource limits to `CircuitReq` to be set
  See [PR 5493](https://github.com/libp2p/rust-libp2p/pull/5493)  


## 0.17.2

- Fix support for unlimited relay connection according to spec.
  See [PR 5244](https://github.com/libp2p/rust-libp2p/pull/5244).
- use `web_time` `Instant` and `SystemTime` versions for wasm support.
  See [PR 5328](https://github.com/libp2p/rust-libp2p/pull/5328).

## 0.17.1

- Automatically register relayed addresses as external addresses.
  See [PR 4809](https://github.com/libp2p/rust-libp2p/pull/4809).
- Fix an error where performing too many reservations at once could lead to inconsistent internal state.
  See [PR 4841](https://github.com/libp2p/rust-libp2p/pull/4841).

## 0.17.0
- Don't close connections on protocol failures within the relay-server.
  To achieve this, error handling was restructured:
  - `libp2p::relay::outbound::stop::FatalUpgradeError` has been removed.
  - `libp2p::relay::outbound::stop::{Error, ProtocolViolation}` have been introduced.
  - Several variants of `libp2p::relay::Event` have been deprecated.

  See [PR 4718](https://github.com/libp2p/rust-libp2p/pull/4718).
- Fix a rare race condition when making a reservation on a relay that could lead to a failed reservation.
  See [PR 4747](https://github.com/libp2p/rust-libp2p/pull/4747).
- Propagate errors of relay client to the listener / dialer.
  A failed reservation will now appear as `SwarmEvent::ListenerClosed` with the `ListenerId` of the corresponding `Swarm::listen_on` call.
  A failed circuit request will now appear as `SwarmEvent::OutgoingConnectionError` with the `ConnectionId` of the corresponding `Swarm::dial` call.
  Lastly, a failed reservation or circuit request will **no longer** close the underlying relay connection.
  As a result, we remove the following enum variants:
  - `relay::client::Event::ReservationReqFailed`
  - `relay::client::Event::OutboundCircuitReqFailed`
  - `relay::client::Event::InboundCircuitReqDenied`
  - `relay::client::Event::InboundCircuitReqDenyFailed`

  See [PR 4745](https://github.com/libp2p/rust-libp2p/pull/4745).

## 0.16.2

## 0.16.1

- Export `RateLimiter` type.
  See [PR 3742].

- Add functions to access data within `Limit`.
  See [PR 4162].

- Remove unconditional `async-std` dependency.
  See [PR 4283].

[PR 3742]: https://github.com/libp2p/rust-libp2p/pull/3742
[PR 4162]: https://github.com/libp2p/rust-libp2p/pull/4162
[PR 4283]: https://github.com/libp2p/rust-libp2p/pull/4283

## 0.16.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Hide internals of `Connection` and expose only `AsyncRead` and `AsyncWrite`.
  See [PR 3829].

- Remove `Event::CircuitReqReceiveFailed` and `Event::InboundCircuitReqFailed` variants.
  These variants are no longer constructed.
  See [PR 3605].

- Remove deprecated items. See [PR 3948].

[PR 3605]: https://github.com/libp2p/rust-libp2p/pull/3605
[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3829]: https://github.com/libp2p/rust-libp2p/pull/3829
[PR 3948]: https://github.com/libp2p/rust-libp2p/pull/3948

## 0.15.2

- Send correct `PeerId` in outbound STOP message to client.
  See [PR 3767].

- As a relay, when forwarding data between relay-connection-source and -destination and vice versa, flush write side when read currently has no more data available.
  See [PR 3765].

[PR 3767]: https://github.com/libp2p/rust-libp2p/pull/3767
[PR 3765]: https://github.com/libp2p/rust-libp2p/pull/3765

## 0.15.1

- Migrate from `prost` to `quick-protobuf`. This removes `protoc` dependency. See [PR 3312].

[PR 3312]: https://github.com/libp2p/rust-libp2p/pull/3312

## 0.15.0

- Rename types as per [discussion 2174].
  `Relay` has been renamed to `Behaviour`.
  The `Relay`, and `Client` prefixes have been removed from various types like `ClientTransport`.
  the `v2` namespace has also been removed, users should prefer importing the relay protocol as a module (`use libp2p::relay;`),
  and refer to its types via `relay::`. For example: `relay::Behaviour` or `relay::client::Behaviour`.
  See [PR 3238].

- Update to `libp2p-core` `v0.39.0`.

- Update to `libp2p-swarm` `v0.42.0`.

[PR 3238]: https://github.com/libp2p/rust-libp2p/pull/3238
[discussion 2174]: https://github.com/libp2p/rust-libp2p/issues/2174

## 0.14.0

- Update to `prost-codec` `v0.3.0`.

- Update to `libp2p-core` `v0.38.0`.

- Update to `libp2p-swarm` `v0.41.0`.

- Replace `Client` and `Relay`'s `NetworkBehaviour` implemention `inject_*` methods with the new `on_*` methods.
  See [PR 3011].

- Replace `client::Handler` and `relay::Handler`'s `ConnectionHandler` implemention `inject_*` methods
  with the new `on_*` methods. See [PR 3085].

- Update `rust-version` to reflect the actual MSRV: 1.62.0. See [PR 3090].

[PR 3085]: https://github.com/libp2p/rust-libp2p/pull/3085
[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.13.0

- Update to `libp2p-core` `v0.37.0`.

- Update to `libp2p-swarm` `v0.40.0`.

- Fix WASM compilation. See [PR 2991].

[PR 2991]: https://github.com/libp2p/rust-libp2p/pull/2991/

## 0.12.0

- Update to `libp2p-swarm` `v0.39.0`.

- Update to `libp2p-core` `v0.36.0`.

## 0.11.0

- Update prost requirement from 0.10 to 0.11 which no longer installs the protoc Protobuf compiler.
  Thus you will need protoc installed locally. See [PR 2788].

- Update to `libp2p-swarm` `v0.38.0`.

- Expose `HOP_PROTOCOL_NAME` and `STOP_PROTOCOL_NAME`. See [PR 2734].

- Update to `libp2p-core` `v0.35.0`.

[PR 2734]: https://github.com/libp2p/rust-libp2p/pull/2734/
[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788

## 0.10.0

- Update to `libp2p-core` `v0.34.0`.

- Update to `libp2p-swarm` `v0.37.0`.

- Do not duplicate the p2p/xxx component with the relay PeerId when a client requests a reservation. See [PR 2701].

- Drive the `RelayListener`s within the `ClientTransport`. Add `Transport::poll` and `Transport::remove_listener`
  for `ClientTransport`. See [PR 2652].

[PR 2701]: https://github.com/libp2p/rust-libp2p/pull/2701/
[PR 2652]: https://github.com/libp2p/rust-libp2p/pull/2652

## 0.9.1

- Respond to at most one incoming reservation request. Deny <= 8 incoming
  circuit requests with one per peer. And deny new circuits before accepting new
  circuits. See [PR 2698].

- Expose explicits errors via `UpgradeError` instead of generic `io::Error`. See
  [PR 2698].

[PR 2698]: https://github.com/libp2p/rust-libp2p/pull/2698/

## 0.9.0

- Update to `libp2p-core` `v0.33.0`.

- Update to `libp2p-swarm` `v0.36.0`.

## 0.8.0

- Expose `{Inbound,Outbound}{Hop,Stop}UpgradeError`. See [PR 2586].

- Update to `libp2p-swarm` `v0.35.0`.

- Remove support for Circuit Relay v1 protocol. See [PR 2549].

[PR 2549]: https://github.com/libp2p/rust-libp2p/pull/2549
[PR 2586]: https://github.com/libp2p/rust-libp2p/pull/2586

## 0.7.0 [2022-02-22]

- Update to `libp2p-core` `v0.32.0`.

- Update to `libp2p-swarm` `v0.34.0`.

- Merge NetworkBehaviour's inject_\* paired methods (see [PR 2445]).

[PR 2445]: https://github.com/libp2p/rust-libp2p/pull/2445

## 0.6.1 [2022-02-02]

- Remove empty peer entries in `reservations` `HashMap`. See [PR 2464].

[PR 2464]: https://github.com/libp2p/rust-libp2p/pull/2464

## 0.6.0 [2022-01-27]

- Update dependencies.

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339

## 0.5.0 [2021-11-16]

- Use `instant` instead of `wasm-timer` (see [PR 2245]).

- Update dependencies.

[PR 2245]: https://github.com/libp2p/rust-libp2p/pull/2245

## 0.4.0 [2021-11-01]

- Make default features of `libp2p-core` optional.
  [PR 2181](https://github.com/libp2p/rust-libp2p/pull/2181)

- Update dependencies.

- Implement `Debug` for `RelayHandlerEvent` and `RelayHandlerIn`. See [PR 2183].

[PR 2183]: https://github.com/libp2p/rust-libp2p/pull/2183

## 0.3.0 [2021-07-12]

- Update dependencies.

## 0.2.0 [2021-04-13]

- Update `libp2p-swarm`.

## 0.1.0 [2021-03-17]

- First release supporting all major features of the circuit relay v1
  specification. [PR 1838](https://github.com/libp2p/rust-libp2p/pull/1838).
