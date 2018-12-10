# Central repository for work on libp2p

<a href="http://libp2p.io/"><img src="https://img.shields.io/badge/project-libp2p-yellow.svg?style=flat-square" /></a>
<a href="http://webchat.freenode.net/?channels=%23libp2p"><img src="https://img.shields.io/badge/freenode-%23libp2p-yellow.svg?style=flat-square" /></a>
[![dependency status](https://deps.rs/repo/github/libp2p/rust-libp2p/status.svg?style=flat-square)](https://deps.rs/repo/github/libp2p/rust-libp2p)

This repository is the central place for Rust development of the [libp2p](https://libp2p.io) spec.

**Warning**: While we are trying our best to be compatible with other lib2p implementations, we
cannot guarantee that it is the case considering the lack of a precise libp2p specifications.

## Documentation

This repository includes a façade crate named `libp2p`, which reexports the rest of the repository.

For documentation, you are encouraged to clone this repository or add `libp2p` as a dependency in
your Cargo.toml and run `cargo doc`.

```toml
[dependencies]
libp2p = { git = "https://github.com/libp2p/rust-libp2p" }
```

## Notable users

(open a pull request if you want your project to be added here)

- https://github.com/paritytech/polkadot
- https://github.com/paritytech/substrate
