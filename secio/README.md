The `secio` protocol is a middleware that will encrypt and decrypt communications going
through a socket (or anything that implements `AsyncRead + AsyncWrite`).

# Connection upgrade

The `SecioConfig` struct implements the `ConnectionUpgrade` trait. You can apply it over a
`Transport` by using the `with_upgrade` method. The returned object will also implement
`Transport` and will automatically apply the secio protocol over any connection that is opened
through it.

```rust
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate libp2p_core;
extern crate libp2p_secio;
extern crate libp2p_tcp_transport;

use futures::Future;
use libp2p_secio::{SecioConfig, SecioKeyPair};
use libp2p_core::{Multiaddr, Transport};
use libp2p_tcp_transport::TcpConfig;
use tokio_core::reactor::Core;
use tokio_io::io::write_all;

let mut core = Core::new().unwrap();

let transport = TcpConfig::new(core.handle())
    .with_upgrade({
        # let private_key = b"";
        //let private_key = include_bytes!("test-rsa-private-key.pk8");
        # let public_key = vec![];
        //let public_key = include_bytes!("test-rsa-public-key.der").to_vec();
        SecioConfig {
            // See the documentation of `SecioKeyPair`.
            key: SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
        }
    });

let future = transport.dial("/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap())
    .unwrap_or_else(|_| panic!("Unable to dial node"))
    .and_then(|(connection, _)| {
        // Sends "hello world" on the connection, will be encrypted.
        write_all(connection, "hello world")
    });

core.run(future).unwrap();
```

# Manual usage

> **Note**: You are encouraged to use `SecioConfig` as described above.

You can add the `secio` layer over a socket by calling `SecioMiddleware::handshake()`. This
method will perform a handshake with the host, and return a future that corresponds to the
moment when the handshake succeeds or errored. On success, the future produces a
`SecioMiddleware` that implements `Sink` and `Stream` and can be used to send packets of data.
