# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.3] - unreleased
### Changed
- Add libp2p-lookup to Dockerfile to enable healthchecks.

## [0.12.2]
### Fixed
- Adhere to `--metrics-path` flag and listen on `0.0.0.0:8888` (default IPFS metrics port).
  [PR 4392]

[PR 4392]: https://github.com/libp2p/rust-libp2p/pull/4392

## [0.12.1]
### Changed
- Move to tokio and hyper.
  See [PR 4311].
- Move to distroless Docker base image.
  See [PR 4311].

[PR 4311]: https://github.com/libp2p/rust-libp2p/pull/4311

## [0.8.0]
### Changed
- Remove mplex support.

## [0.7.0]
### Changed
- Update to libp2p v0.47.0.

## [0.6.0] - [2022-05-05]
### Changed
- Update to libp2p v0.44.0.

## [0.5.4] - [2022-01-11]
### Changed
- Pull latest autonat changes.

## [0.5.3] - [2021-12-25]
### Changed
- Update dependencies.
- Pull in autonat fixes.

## [0.5.2] - [2021-12-20]
### Added
- Add support for libp2p autonat protocol via `--enable-autonat`.

## [0.5.1] - [2021-12-20]
### Fixed
- Update dependencies.
- Fix typo in command line flag `--enable-kademlia`.

## [0.5.0] - 2021-11-18
### Changed
- Disable Kademlia protocol by default.

## [0.4.0] - 2021-11-18
### Fixed
- Update dependencies.
