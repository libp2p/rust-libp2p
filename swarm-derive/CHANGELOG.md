## 0.34.1

- Always forward all variants of `FromSwarm`.
  See [PR 4825](https://github.com/libp2p/rust-libp2p/pull/4825).

## 0.34.0

- Adapt to interface changes in `libp2p-swarm`.
  See [PR 4706](https://github.com/libp2p/rust-libp2p/pull/4076).
- Remove supported for deprecated `#[behaviour(out_event = "...")]`.
  To same functionality is available using `#[behaviour(to_swarm = "...")]`
  See [PR 4737](https://github.com/libp2p/rust-libp2p/pull/4737).

## 0.33.0

- Raise MSRV to 1.65.
  See [PR 3715].

- Rename `out_event` to `to_swarm` and deprecate `out_event`. See [PR 3848].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3848]: https://github.com/libp2p/rust-libp2p/pull/3848

## 0.32.0

- Fix `NetworkBehaviour` Derive macro for generic types when `out_event` was not provided. Previously the enum generated
  didn't have the `NetworkBehaviour` impl constraints whilst using the generics for `<Generic>::OutEvent`.
  See [PR 3393].

- Replace `NetworkBehaviour` Derive macro deprecated `inject_*` method implementations
  with the new `on_swarm_event` and `on_connection_handler_event`.
  See [PR 3011] and [PR 3264].

[PR 3393]: https://github.com/libp2p/rust-libp2p/pull/3393
[PR 3011]: https://github.com/libp2p/rust-libp2p/pull/3011
[PR 3264]: https://github.com/libp2p/rust-libp2p/pull/3264

## 0.31.0

- Add `prelude` configuration option.
  The derive-macro generates code that needs to refer to various symbols. See [PR 3055].

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 3055]: https://github.com/libp2p/rust-libp2p/pull/3055
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

## 0.30.1

- Fix an issue where the derive would generate bad code if the type parameters between the behaviour and a custom
  out event differed. See [PR 2907].
- Fix an issue where the derive would generate incorrect code depending on available imports. See [PR 2921].

[PR 2907]: https://github.com/libp2p/rust-libp2p/pull/2907
[PR 2921]: https://github.com/libp2p/rust-libp2p/pull/2921

## 0.30.0

- Remove support for removed `NetworkBehaviourEventProcess`. See [PR 2840].

- Remove support for custom `poll` method on `NetworkBehaviour` via `#[behaviour(poll_method =
  "poll")]`. See [PR 2841].

[PR 2840]: https://github.com/libp2p/rust-libp2p/pull/2840
[PR 2841]: https://github.com/libp2p/rust-libp2p/pull/2841

- Remove support for non-`NetworkBehaviour` fields on main `struct` via `#[behaviour(ignore)]`. See
  [PR 2842].

[PR 2842]: https://github.com/libp2p/rust-libp2p/pull/2842

## 0.29.0

- Generate `NetworkBehaviour::OutEvent` if not provided through `#[behaviour(out_event =
  "MyOutEvent")]` and event processing is disabled (default).

## 0.28.0

- Import `ListenerId` from `libp2p::core::transport`. See [PR 2652].

[PR 2652]: https://github.com/libp2p/rust-libp2p/pull/2652

## 0.27.2

- Replace references of Protocol Handler with Connection Handler. See [PR 2640].

[PR 2640]: https://github.com/libp2p/rust-libp2p/pull/2640

## 0.27.1

- Allow mixing of ignored fields. See [PR 2570].

[PR 2570]: https://github.com/libp2p/rust-libp2p/pull/2570

## 0.27.0 [2022-02-22]

- Adjust to latest changes in `libp2p-swarm`.

## 0.26.1 [2022-01-27]

- Remove unnecessary clone of error in `inject_dial_failure` (see [PR 2349]).

- Migrate to Rust edition 2021 (see [PR 2339]).

[PR 2339]: https://github.com/libp2p/rust-libp2p/pull/2339
[PR 2349]: https://github.com/libp2p/rust-libp2p/pull/2349

## 0.26.0 [2021-11-16]

- Adjust to advanced dialing requests API changes (see [PR 2317]).

[PR 2317]: https://github.com/libp2p/rust-libp2p/pull/2317

## 0.25.0 [2021-11-01]

- Update to latest `libp2p-swarm` changes (see [PR 2191]).

- Make `event_process = false` the default.

[PR 2191]: https://github.com/libp2p/rust-libp2p/pull/2191

## 0.24.0 [2021-07-12]

- Handle `NetworkBehaviourAction::CloseConnection`. See [PR 2110] for details.

[PR 2110]: https://github.com/libp2p/rust-libp2p/pull/2110/

## 0.23.0 [2021-04-14]

- Extend `NetworkBehaviour` callbacks, more concretely introducing new `fn
  inject_new_listener` and `fn inject_expired_external_addr` and have `fn
  inject_{new,expired}_listen_addr` provide a `ListenerId` [PR
  2011](https://github.com/libp2p/rust-libp2p/pull/2011).

## 0.22.0 [2021-02-15]

- Rename the crate to `libp2p-swarm-derive`.

## 0.21.0 [2020-11-25]

- Update for compatibility with `libp2p-swarm-0.25`.

## 0.20.2 [2020-07-28]

- Generate fully-qualified method name for `poll` to avoid
ambiguity. [PR 1681](https://github.com/libp2p/rust-libp2p/pull/1681).

## 0.20.1 [2020-07-08]

- Allow users to opt out of the `NetworkBehaviourEventProcess`
mechanism through `#[behaviour(event_process = false)]`. This is
useful if users want to process all events while polling the
swarm through `SwarmEvent::Behaviour`.
