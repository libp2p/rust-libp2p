# futures-mutex

*A Mutex for the Future(s)*

[![Crates.io](https://img.shields.io/crates/v/futures-mutex.svg)](https://crates.io/crates/futures-mutex)
[![Docs.rs](https://docs.rs/futures-mutex/badge.svg)](https://docs.rs/futures-mutex)

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
futures-mutex = "0.2.0"
```

Then, add this to your crate:

```rust
extern crate futures_mutex;
```

`FutMutex<T>` follows a very similar API to  [`futures::sync::BiLock`](https://docs.rs/futures/0.1.11/futures/sync/struct.BiLock.html), however it can have more than two handles.

## License

`futures-mutex` is distributed under the Apache License v2.0. See the LICENSE
file for the full text of the license.
