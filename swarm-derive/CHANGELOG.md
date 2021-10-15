# 0.25.0-rc.1 [2021-10-15]

- Update to latest `libp2p-swarm` changes (see [PR 2191]).

- Make `event_process = false` the default.

[PR 2191]: https://github.com/libp2p/rust-libp2p/pull/2191

# 0.24.0 [2021-07-12]

- Handle `NetworkBehaviourAction::CloseConnection`. See [PR 2110] for details.

[PR 2110]: https://github.com/libp2p/rust-libp2p/pull/2110/

# 0.23.0 [2021-04-14]

- Extend `NetworkBehaviour` callbacks, more concretely introducing new `fn
  inject_new_listener` and `fn inject_expired_external_addr` and have `fn
  inject_{new,expired}_listen_addr` provide a `ListenerId` [PR
  2011](https://github.com/libp2p/rust-libp2p/pull/2011).

# 0.22.0 [2021-02-15]

- Rename the crate to `libp2p-swarm-derive`.

# 0.21.0 [2020-11-25]

- Update for compatibility with `libp2p-swarm-0.25`.

# 0.20.2 [2020-07-28]

- Generate fully-qualified method name for `poll` to avoid
ambiguity. [PR 1681](https://github.com/libp2p/rust-libp2p/pull/1681).

# 0.20.1 [2020-07-08]

- Allow users to opt out of the `NetworkBehaviourEventProcess`
mechanism through `#[behaviour(event_process = false)]`. This is
useful if users want to process all events while polling the
swarm through `SwarmEvent::Behaviour`.
