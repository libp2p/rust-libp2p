## 0.11.2

- Deprecate `void` crate.
  See [PR 5676](https://github.com/libp2p/rust-libp2p/pull/5676).

## 0.11.1

- Update `libp2p-tls` to version `0.5.0`, see [PR 5547]

[PR 5547]: https://github.com/libp2p/rust-libp2p/pull/5547

## 0.11.0

- Implement refactored `Transport`.
  See [PR 4568](https://github.com/libp2p/rust-libp2p/pull/4568)

## 0.10.3

- Update `quinn` to 0.11 and `libp2p-tls` to 0.4.0.
  See [PR 5316](https://github.com/libp2p/rust-libp2p/pull/5316)

- Allow configuring MTU discovery upper bound.
  See [PR 5386](https://github.com/libp2p/rust-libp2p/pull/5386).

## 0.10.2

- Change `max_idle_timeout`to 10s.
  See [PR 4965](https://github.com/libp2p/rust-libp2p/pull/4965).

## 0.10.1

- Allow disabling path MTU discovery.
  See [PR 4823](https://github.com/libp2p/rust-libp2p/pull/4823).

## 0.10.0

- Improve hole-punch timing.
  This should improve success rates for hole-punching QUIC connections.
  See [PR 4549](https://github.com/libp2p/rust-libp2p/pull/4549).
- Remove deprecated `Error::EndpointDriverCrashed` variant.
  See [PR 4738](https://github.com/libp2p/rust-libp2p/pull/4738).

## 0.9.3

- No longer report error when explicit closing of a QUIC endpoint succeeds.
  See [PR 4621].

- Support QUIC stateless resets for supported `libp2p_identity::Keypair`s. See [PR 4554].

[PR 4621]: https://github.com/libp2p/rust-libp2p/pull/4621
[PR 4554]: https://github.com/libp2p/rust-libp2p/pull/4554

## 0.9.2

- Cut stable release.

## 0.9.2-alpha

- Add support for reusing an existing socket when dialing localhost address.
  See [PR 4304].

[PR 4304]: https://github.com/libp2p/rust-libp2p/pull/4304

## 0.9.1-alpha

- Allow listening on ipv4 and ipv6 separately.
  See [PR 4289].

[PR 4289]: https://github.com/libp2p/rust-libp2p/pull/4289

## 0.9.0-alpha

- Use `quinn` instead of `quinn-proto`.
  See [PR 3454].

[PR 3454]: https://github.com/libp2p/rust-libp2p/pull/3454

## 0.8.0-alpha

- Raise MSRV to 1.65.
  See [PR 3715].

- Add hole punching support by implementing `Transport::dial_as_listener`. See [PR 3964].

[PR 3715]: https://github.com/libp2p/rust-libp2p/pull/3715
[PR 3964]: https://github.com/libp2p/rust-libp2p/pull/3964

## 0.7.0-alpha.3

- Depend `libp2p-tls` `v0.1.0`.

## 0.7.0-alpha.2

- Update to `libp2p-tls` `v0.1.0-alpha.2`.

- Update to `libp2p-core` `v0.39.0`.

- Add opt-in support for the `/quic` codepoint, interpreted as QUIC version draft-29.
  See [PR 3151].

- Wake the transport's task when a new dialer or listener is added. See [3342].

- Discard correct waker upon accepting inbound stream. See [PR 3420].

[PR 3151]: https://github.com/libp2p/rust-libp2p/pull/3151
[PR 3342]: https://github.com/libp2p/rust-libp2p/pull/3342
[PR 3420]: https://github.com/libp2p/rust-libp2p/pull/3420

## 0.7.0-alpha

- Initial alpha release.
