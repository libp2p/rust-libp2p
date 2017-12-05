Transport and protocol upgrade system of *libp2p*.

This crate contains all the core traits and mechanisms of the transport system of *libp2p*.

# The `Transport` trait

The main trait that this crate provides is `Transport`, which provides the `dial` and
`listen_on` methods and can be used to dial or listen on a multiaddress. This crate does not
provide any concrete (non-dummy, non-adapter) implementation of this trait. It is implemented
on structs such as `TcpConfig`, `UdpConfig`, `WebsocketConfig` that are provided by external
crates.

Each implementation of `Transport` only supports *some* multiaddress protocols, for example
the `TcpConfig` struct only supports multiaddresses that look like `/ip*/*.*.*.*/tcp/*`. Two
implementations of `Transport` can be grouped together with the `or_transport` method, in order
to obtain a single object that supports the protocols of both objects at once. This can be done
multiple times in order to chain as many implementations as you want.

// TODO: right now only tcp-transport exists, but we need to add an example for chaining multiple
//       transports once that makes sense

# Connection upgrades

Once a socket has been opened with a remote through a `Transport`, it can be *upgraded*. This
consists in negotiating a protocol with the remote (through `multistream-select`), and applying
that protocol on the socket.

A potential connection upgrade is represented with the `ConnectionUpgrade` trait. The trait
consists in a protocol name plus a method that turns the socket into an `Output` object whose
nature and type is specific to each upgrade.

There exists two kinds of connection upgrades: middlewares, and actual protocols.

## Middlewares

Examples of middleware protocol upgrades include `PlainTextConfig` (dummy upgrade),
`SecioConfig` (encyption layer), or `MultiplexConfig`.

The output of a middleware protocol upgrade must implement the `AsyncRead` and `AsyncWrite`
traits, just like sockets do.

A middleware can be applied by using the `with_upgrade` method of the `Transport` trait. The
return value of this method also implements the `Transport` trait, which means that you can
call `dial()` and `listen_on()` on it in order to directly obtain an upgraded connection. An
error is produced if the remote doesn't support the protocol that you are trying to negotiate.

```
extern crate libp2p_swarm;
extern crate libp2p_tcp_transport;
extern crate tokio_core;

use libp2p_swarm::Transport;

let tokio_core = tokio_core::reactor::Core::new().unwrap();
let tcp_transport = libp2p_tcp_transport::Tcp::new(tokio_core.handle()).unwrap();
let upgraded = tcp_transport.with_upgrade(libp2p_swarm::PlainText);

// upgraded.dial(...)   // automatically applies the plain text protocol on the socket
```

## Actual protocols

*Actual protocols* work the same way as middlewares, except that their `Output` doesn't
implement the `AsyncRead` and `AsyncWrite` traits. The consequence of this is that the return
value of `with_upgrade` **cannot** doesn't implement the `Transport` trait and thus cannot be
used as a transport.

However the value returned by `with_upgrade` still provides methods named `dial` and
`listen_on`, which will yield you respectively a `Future` or a `Stream`, which you can use to
obtain the `Output`. This `Output` can then be used in a protocol-specific way to use the
protocol.

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

let ping_finished_future = libp2p_tcp_transport::Tcp::new(core.handle()).unwrap()
    // We have a `Tcp` struct that implements `Transport`, and apply a `Ping` upgrade on it.
    .with_upgrade(Ping)
    // TODO: right now the only available protocol is ping, but we want to replace it with
    //       something that is more simple to use
    .dial(libp2p_swarm::Multiaddr::new("127.0.0.1:12345").unwrap()).unwrap_or_else(|_| panic!())
    .and_then(|(mut pinger, service)| {
        pinger.ping().map_err(|_| panic!()).select(service).map_err(|_| panic!())
    });

// Runs until the ping arrives.
core.run(ping_finished_future).unwrap();
```

## Grouping protocols

You can use the `.or_upgrade()` method to group multiple upgrades together. The return value
also implements the `ConnectionUpgrade` trait and will choose one of the protocols amongst the
ones supported.

