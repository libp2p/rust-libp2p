# rust-cid

[![](https://img.shields.io/badge/made%20by-Protocol%20Labs-blue.svg?style=flat-square)](http://ipn.io)
[![](https://img.shields.io/badge/project-ipld-blue.svg?style=flat-square)](https://github.com/ipld/ipld)
[![](https://img.shields.io/badge/freenode-%23ipfs-blue.svg?style=flat-square)](https://webchat.freenode.net/?channels=%23ipfs)
[![Travis CI](https://img.shields.io/travis/ipld/rust-cid.svg?style=flat-square&branch=master)](https://travis-ci.org/ipld/rust-cid)
[![](https://img.shields.io/badge/rust-docs-blue.svg?style=flat-square)](https://docs.rs/crate/cid)
[![crates.io](https://img.shields.io/badge/crates.io-v0.1.0-orange.svg?style=flat-square )](https://crates.io/crates/cid)
[![](https://img.shields.io/badge/readme%20style-standard-brightgreen.svg?style=flat-square)](https://github.com/RichardLitt/standard-readme)

> [CID](https://github.com/ipld/cid) implementation in Rust.

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
cid = "*"
```

Then run `cargo build`.

## Usage

```rust
extern crate cid;
extern crate multihash;

use multihash::Hash;
use cid::{Cid, Codec, Version};
let h = multihash::encode(multihash::Hash::SHA2256, b"beep boop").unwrap();

let cid = Cid::new(Codec::DagProtobuf, Version::V1, &h);

let data = cid.to_bytes();
let out = Cid::from(data).unwrap();

assert_eq!(cid, out);
```
## Maintainers

Captain: [@dignifiedquire](https://github.com/dignifiedquire).

## Contribute

Contributions welcome. Please check out [the issues](https://github.com/ipld/rust-cid/issues).

Check out our [contributing document](https://github.com/ipld/ipld/blob/master/contributing.md) for more information on how we work, and about contributing in general. Please be aware that all interactions related to ipld are subject to the IPFS [Code of Conduct](https://github.com/ipfs/community/blob/master/code-of-conduct.md).

Small note: If editing the README, please conform to the [standard-readme](https://github.com/RichardLitt/standard-readme) specification.


## License

[MIT](LICENSE) © 2017 Friedel Ziegelmayer
