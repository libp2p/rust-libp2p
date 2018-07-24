# TCP transport

Implementation of the libp2p `Transport` trait for TCP/IP.

Uses [the *tokio* library](https://tokio.rs).

## Usage

Example:

```rust
extern crate libp2p_tcp_transport;
extern crate tokio_current_thread;

use libp2p_tcp_transport::TcpConfig;
use tokio_core::reactor::Core;

let mut core = Core::new().unwrap();
let tcp = TcpConfig::new();
```

The `TcpConfig` structs implements the `Transport` trait of the `swarm` library. See the
documentation of `swarm` and of libp2p in general to learn how to use the `Transport` trait.