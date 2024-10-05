## 0.12.8

### Changed

- Remove deprecated [`libp2p-lookup`](https://github.com/mxinden/libp2p-lookup) from Dockerfile.
  See [PR 5610](https://github.com/libp2p/rust-libp2p/pull/5610).

## 0.12.7

### Changed

- Use periodic and automatic bootstrap of Kademlia.
  See [PR 4838](https://github.com/libp2p/rust-libp2p/pull/4838).
- Update to [`libp2p-identify` `v0.45.0`](protocols/identify/CHANGELOG.md#0450).
  See [PR 4981](https://github.com/libp2p/rust-libp2p/pull/4981).

## 0.12.6

### Changed

- Stop using kad default protocol.
  See [PR 5122](https://github.com/libp2p/rust-libp2p/pull/5122)

## 0.12.5

### Added

- Add `/wss` support.
  See [PR 4937](https://github.com/libp2p/rust-libp2p/pull/4937).

## 0.12.4

### Added

- Expose `libp2p_bandwidth_bytes` Prometheus metrics.
  See [PR 4727](https://github.com/libp2p/rust-libp2p/pull/4727).

## 0.12.3

### Changed

- Add libp2p-lookup to Dockerfile to enable healthchecks.

### Fixed

- Disable QUIC `draft-29` support.
  Listening on `/quic` and `/quic-v1` addresses with the same port would otherwise result in an "Address already in use" error by the OS.
  See [PR 4467].

[PR 4467]: https://github.com/libp2p/rust-libp2p/pull/4467

## 0.12.2

### Fixed

- Adhere to `--metrics-path` flag and listen on `0.0.0.0:8888` (default IPFS metrics port).
  [PR 4392]

[PR 4392]: https://github.com/libp2p/rust-libp2p/pull/4392

## 0.12.1

### Changed

- Move to tokio and hyper.
  See [PR 4311].
- Move to distroless Docker base image.
  See [PR 4311].

[PR 4311]: https://github.com/libp2p/rust-libp2p/pull/4311

## 0.8.0

### Changed

- Remove mplex support.

## 0.7.0

### Changed

- Update to libp2p v0.47.0.

## 0.6.0 - 2022-05-05

### Changed

- Update to libp2p v0.44.0.

## 0.5.4 - 2022-01-11

### Changed

- Pull latest autonat changes.

## 0.5.3 - 2021-12-25

### Changed

- Update dependencies.
- Pull in autonat fixes.

## 0.5.2 - 2021-12-20

### Added

- Add support for libp2p autonat protocol via `--enable-autonat`.

## 0.5.1 - 2021-12-20

### Fixed

- Update dependencies.
- Fix typo in command line flag `--enable-kademlia`.

## 0.5.0 - 2021-11-18

### Changed

- Disable Kademlia protocol by default.

## 0.4.0 - 2021-11-18

### Fixed

- Update dependencies.
