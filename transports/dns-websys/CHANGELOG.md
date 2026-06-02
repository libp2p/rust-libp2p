## 0.1.0

- Support DNS transport for wasm32 targets that resolves DNS components over DNS-over-HTTPS (DoH).
  `/dnsaddr` is always resolved, however `/dns`, `/dns4` and `/dns6` are governed by the
  `DnsResolution` policy (default to `DnsResolutionAuto`): addresses containing a explicit protocols
  (i.e. `webrtc-direct`) are resolved to `/ip4`/`/ip6`, while the rest are passed through to the inner
  transport unchanged, since browsers resolve those hostnames natively and need
  the hostname preserved for SNI.
  See [PR XXXX](https://github.com/libp2p/rust-libp2p/pull/XXXX).