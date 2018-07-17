Implementation of the libp2p `Transport` trait for Websockets.

See the documentation of `swarm` and of libp2p in general to learn how to use the `Transport`
trait.

This library is used in a different way depending on whether you are compiling for emscripten
or for a different operating system.

# Emscripten

On emscripten, you can create a `BrowserWsConfig` object with `BrowserWsConfig::new()`. It can
then be used as a transport.

Listening on a websockets multiaddress isn't supported on emscripten. Dialing a multiaddress
which uses `ws` on top of TCP/IP will automatically use the `XMLHttpRequest` Javascript object.

```rust
use libp2p_websocket::BrowserWsConfig;

let ws_config = BrowserWsConfig::new();
// let _ = ws_config.dial("/ip4/40.41.42.43/tcp/12345/ws".parse().unwrap());
```

# Other operating systems

On other operating systems, this library doesn't open any socket by itself. Instead it must be
plugged on top of another implementation of `Transport` such as TCP/IP.

This underlying transport must be put inside a `WsConfig` object through the
`WsConfig::new()` function.

```rust
extern crate libp2p_core;
extern crate libp2p_tcp_transport;
extern crate libp2p_websocket;
extern crate tokio_current_thread;

use libp2p_core::{Multiaddr, Transport};
use libp2p_tcp_transport::TcpConfig;
use libp2p_websocket::WsConfig;
use tokio_core::reactor::Core;

let core = Core::new().unwrap();
let ws_config = WsConfig::new(TcpConfig::new());
let _ = ws_config.dial("/ip4/40.41.42.43/tcp/12345/ws".parse().unwrap());
```
