Handles the `/ipfs/ping/1.0.0` protocol. This allows pinging a remote node and waiting for an
answer.

# Usage

Create a `Ping` struct, which implements the `ConnectionUpgrade` trait. When used as a
connection upgrade, it will produce a tuple of type `(Pinger, impl Future<Item = ()>)` which
are named the *pinger* and the *ponger*.

The *pinger* has a method named `ping` which will send a ping to the remote, while the *ponger*
is a future that will process the data received on the socket and will be signalled only when
the connection closes.

# About timeouts

For technical reasons, this crate doesn't handle timeouts. The action of pinging returns a
future that is signalled only when the remote answers. If the remote is not responsive, the
future will never be signalled.

For implementation reasons, resources allocated for a ping are only ever fully reclaimed after
a pong has been received by the remote. Therefore if you repeatidely ping a non-responsive
remote you will end up using more and memory memory (albeit the amount is very very small every
time), even if you destroy the future returned by `ping`.

This is probably not a problem in practice, because the nature of the ping protocol is to
determine whether a remote is still alive, and any reasonable user of this crate will close
connections to non-responsive remotes.

# Example

```rust
extern crate futures;
extern crate libp2p_ping;
extern crate libp2p_swarm;
extern crate libp2p_tcp_transport;
extern crate tokio_core;

use futures::Future;
use libp2p_ping::Ping;
use libp2p_swarm::Transport;

let mut core = tokio_core::reactor::Core::new().unwrap();

let ping_finished_future = libp2p_tcp_transport::TcpConfig::new(core.handle())
    .with_upgrade(Ping)
    .dial("127.0.0.1:12345".parse::<libp2p_swarm::Multiaddr>().unwrap()).unwrap_or_else(|_| panic!())
    .and_then(|((mut pinger, service), _)| {
        pinger.ping().map_err(|_| panic!()).select(service).map_err(|_| panic!())
    });

// Runs until the ping arrives.
core.run(ping_finished_future).unwrap();
```

