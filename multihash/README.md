# rust-multihash

[![](https://img.shields.io/badge/made%20by-Protocol%20Labs-blue.svg?style=flat-square)](http://ipn.io)
[![](https://img.shields.io/badge/project-multiformats-blue.svg?style=flat-square)](https://github.com/multiformats/multiformats)
[![](https://img.shields.io/badge/freenode-%23ipfs-blue.svg?style=flat-square)](https://webchat.freenode.net/?channels=%23ipfs)
[![Travis CI](https://img.shields.io/travis/multiformats/rust-multihash.svg?style=flat-square&branch=master)](https://travis-ci.org/multiformats/rust-multihash)
[![codecov.io](https://img.shields.io/codecov/c/github/multiformats/rust-multihash.svg?style=flat-square&branch=master)](https://codecov.io/github/multiformats/rust-multihash?branch=master)
[![](https://img.shields.io/badge/rust-docs-blue.svg?style=flat-square)](https://docs.rs/multihash/)
[![crates.io](https://img.shields.io/badge/crates.io-v0.4.0-orange.svg?style=flat-square )](https://crates.io/crates/multihash)
[![](https://img.shields.io/badge/readme%20style-standard-brightgreen.svg?style=flat-square)](https://github.com/RichardLitt/standard-readme)

> [multihash](https://github.com/multiformats/multihash) implementation in Rust.

## Table of Contents

- [Install](#install)
- [Usage](#usage)
- [Supported Hash Types](#supported-hash-types)
- [Dependencies](#dependencies)
- [Maintainers](#maintainers)
- [Contribute](#contribute)
- [License](#license)

## Install

First add this to your `Cargo.toml`

```toml
[dependencies]
multihash = "*"
```

Then run `cargo build`.

## Usage

```rust
extern crate multihash;

use multihash::{encode, decode, Hash};

let hash = encode(Hash::SHA2256, b"my hash").unwrap();
let multi = decode(&hash).unwrap();
```

## Supported Hash Types

* `SHA1`
* `SHA2-256`
* `SHA2-512`
* `SHA3`/`Keccak`

## Maintainers

Captain: [@dignifiedquire](https://github.com/dignifiedquire).

## Contribute

Contributions welcome. Please check out [the issues](https://github.com/multiformats/rust-multihash/issues).

Check out our [contributing document](https://github.com/multiformats/multiformats/blob/master/contributing.md) for more information on how we work, and about contributing in general. Please be aware that all interactions related to multiformats are subject to the IPFS [Code of Conduct](https://github.com/ipfs/community/blob/master/code-of-conduct.md).

Small note: If editing the README, please conform to the [standard-readme](https://github.com/RichardLitt/standard-readme) specification.


## License

[MIT](LICENSE) © 2015-2017 Friedel Ziegelmayer
