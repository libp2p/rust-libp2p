# 0.3.0

- Implement `From` trait for `std::io::Error`. See [PR 2622].
- Don't leak `prost` dependency in `Error` type. See [PR 3058].

- Update `rust-version` to reflect the actual MSRV: 1.60.0. See [PR 3090].

[PR 2622]: https://github.com/libp2p/rust-libp2p/pull/2622/
[PR 3058]: https://github.com/libp2p/rust-libp2p/pull/3058/
[PR 3090]: https://github.com/libp2p/rust-libp2p/pull/3090

# 0.2.0

- Update to prost(-build) `v0.11`. See [PR 2788].

[PR 2788]: https://github.com/libp2p/rust-libp2p/pull/2788/
