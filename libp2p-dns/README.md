# TCP transport

Implementation of the libp2p `Transport` trait for TCP/IP.

Uses [the *tokio* library](https://tokio.rs).

## Usage

Create [a tokio `Core`](https://docs.rs/tokio-core/0.1/tokio_core/reactor/struct.Core.html),
then grab a handle by calling the `handle()` method on it, then create a `TcpConfig` and pass
the handle.

Example:

```rust
extern crate libp2p_tcp_transport;
extern crate tokio_core;

use libp2p_tcp_transport::TcpConfig;
use tokio_core::reactor::Core;

let mut core = Core::new().unwrap();
let tcp = TcpConfig::new(core.handle());
```

The `TcpConfig` structs implements the `Transport` trait of the `swarm` library. See the
documentation of `swarm` and of libp2p in general to learn how to use the `Transport` trait.