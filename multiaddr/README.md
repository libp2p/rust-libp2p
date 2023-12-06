# rust-multiaddr

[![](https://img.shields.io/badge/made%20by-Protocol%20Labs-blue.svg?style=flat-square)](http://ipn.io)
[![](https://img.shields.io/badge/project-multiformats-blue.svg?style=flat-square)](https://github.com/multiformats/multiformats)
[![](https://img.shields.io/badge/freenode-%23ipfs-blue.svg?style=flat-square)](https://webchat.freenode.net/?channels=%23ipfs)
[![Travis CI](https://img.shields.io/travis/multiformats/rust-multiaddr.svg?style=flat-square&branch=master)](https://travis-ci.org/multiformats/rust-multiaddr)
[![codecov.io](https://img.shields.io/codecov/c/github/multiformats/rust-multiaddr.svg?style=flat-square&branch=master)](https://codecov.io/github/multiformats/rust-multiaddr?branch=master)
[![](https://img.shields.io/badge/rust-docs-blue.svg?style=flat-square)](https://docs.rs/crate/multiaddr)
[![crates.io](https://img.shields.io/badge/crates.io-v0.2.0-orange.svg?style=flat-square )](https://crates.io/crates/multiaddr)
[![](https://img.shields.io/badge/readme%20style-standard-brightgreen.svg?style=flat-square)](https://github.com/RichardLitt/standard-readme)


> [multiaddr](https://github.com/multiformats/multiaddr) implementation in Rust.

## Table of Contents

- [Install](#install)
- [Usage](#usage)
- [Maintainers](#maintainers)
- [Contribute](#contribute)
- [License](#license)

## Install

First add this to your `Cargo.toml`

```toml
[dependencies]
multiaddr = "*"
```

then run `cargo build`.

## Usage

```rust
extern crate multiaddr;

use multiaddr::{Multiaddr, multiaddr};

let address = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap();
// or with a macro
let other = multiaddr!(Ip4([127, 0, 0, 1]), Udp(10500u16), QuicV1);

assert_eq!(address.to_string(), "/ip4/127.0.0.1/tcp/1234");
assert_eq!(other.to_string(), "/ip4/127.0.0.1/udp/10500/quic-v1");
```

## Maintainers

Captain: [@dignifiedquire](https://github.com/dignifiedquire).

## Contribute

Contributions welcome. Please check out [the issues](https://github.com/multiformats/rust-multiaddr/issues).

Check out our [contributing document](https://github.com/multiformats/multiformats/blob/master/contributing.md) for more information on how we work, and about contributing in general. Please be aware that all interactions related to multiformats are subject to the IPFS [Code of Conduct](https://github.com/ipfs/community/blob/master/code-of-conduct.md).

Small note: If editing the README, please conform to the [standard-readme](https://github.com/RichardLitt/standard-readme) specification.

## License

[MIT](LICENSE) © 2015-2017 Friedel Ziegelmeyer
