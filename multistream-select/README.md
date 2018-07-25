# Multistream-select

This crate implements the `multistream-select` protocol, which is the protocol used by libp2p
to negotiate which protocol to use with the remote.

> **Note**: This crate is used by the internals of *libp2p*, and it is not required to
> understand it in order to use *libp2p*.

Whenever a new connection or a new multiplexed substream is opened, libp2p uses
`multistream-select` to negotiate with the remote which protocol to use. After a protocol has
been successfully negotiated, the stream (ie. the connection or the multiplexed substream)
immediately stops using `multistream-select` and starts using the negotiated protocol.

## Protocol explanation

The dialer has two options available: either request the list of protocols that the listener
supports, or suggest a protocol. If a protocol is suggested, the listener can either accept (by
answering with the same protocol name) or refuse the choice (by answering "not available").

## Examples

For a dialer:

```rust
extern crate bytes;
extern crate futures;
extern crate multistream_select;
extern crate tokio_current_thread;

use bytes::Bytes;
use multistream_select::dialer_select_proto;
use futures::{Future, Sink, Stream};
use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;

let mut core = Core::new().unwrap();

#[derive(Debug, Copy, Clone)]
enum MyProto { Echo, Hello }

let client = TcpStream::connect(&"127.0.0.1:10333".parse().unwrap(), &core.handle())
    .from_err()
    .and_then(move |connec| {
        let protos = vec![
            (Bytes::from("/echo/1.0.0"), <Bytes as PartialEq>::eq, MyProto::Echo),
            (Bytes::from("/hello/2.5.0"), <Bytes as PartialEq>::eq, MyProto::Hello),
        ]
                        .into_iter();
        dialer_select_proto(connec, protos).map(|r| r.0)
    });

let negotiated_protocol: MyProto = tokio_current_thread::block_on_all(client).expect("failed to find a protocol");
println!("negotiated: {:?}", negotiated_protocol);
```

For a listener:

```rust
extern crate bytes;
extern crate futures;
extern crate multistream_select;
extern crate tokio_current_thread;

use bytes::Bytes;
use multistream_select::listener_select_proto;
use futures::{Future, Sink, Stream};
use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;

let mut core = Core::new().unwrap();

#[derive(Debug, Copy, Clone)]
enum MyProto { Echo, Hello }

let server = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap()
    .incoming()
    .from_err()
    .and_then(move |(connec, _)| {
        let protos = vec![
            (Bytes::from("/echo/1.0.0"), <Bytes as PartialEq>::eq, MyProto::Echo),
            (Bytes::from("/hello/2.5.0"), <Bytes as PartialEq>::eq, MyProto::Hello),
        ]
                        .into_iter();
        listener_select_proto(connec, protos)
    })
    .for_each(|(proto, _connec)| {
        println!("new remote with {:?} negotiated", proto);
        Ok(())
    });

tokio_current_thread::block_on_all(server).expect("failed to run server");
```
