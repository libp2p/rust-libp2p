// Copyright 2017 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

extern crate bytes;
extern crate futures;
extern crate libp2p_secio as secio;
extern crate libp2p_swarm as swarm;
extern crate libp2p_tcp_transport as tcp;
extern crate multiplex;
extern crate tokio_core;
extern crate tokio_io;

use bytes::BytesMut;
use futures::{Future, Sink, Stream};
use std::env;
use swarm::{UpgradeExt, SimpleProtocol, Transport, MuxedTransport};
use tcp::TcpConfig;
use tokio_core::reactor::Core;
use tokio_io::codec::length_delimited;

fn main() {
    // Determine which address to dial.
    let target_addr = env::args().nth(1).unwrap_or("/ip4/127.0.0.1/tcp/10333".to_owned());

    // We start by building the tokio engine that will run all the sockets.
    let mut core = Core::new().unwrap();

    // Now let's build the transport stack.
    // We start by creating a `TcpConfig` that indicates that we want TCP/IP.
    let transport = TcpConfig::new(core.handle())

        // On top of TCP/IP, we will use either the plaintext protocol or the secio protocol,
        // depending on which one the remote supports.
        .with_upgrade({
            let plain_text = swarm::PlainTextConfig;

            let secio = {
                let private_key = include_bytes!("test-private-key.pk8");
                let public_key = include_bytes!("test-public-key.der").to_vec();
                secio::SecioConfig {
                    key: secio::SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
                }
            };

            plain_text.or_upgrade(secio)
        })

        // On top of plaintext or secio, we will use the multiplex protocol.
        .with_upgrade(multiplex::MultiplexConfig)
        // The object returned by the call to `with_upgrade(MultiplexConfig)` can't be used as a
        // `Transport` because the output of the upgrade is not a stream but a controller for
        // muxing. We have to explicitly call `into_connection_reuse()` in order to turn this into
        // a `Transport`.
        .into_connection_reuse();

    let transport_with_echo = transport
        .clone()
        // On top of plaintext or secio, we use the "echo" protocol, which is a custom protocol
        // just for this example.
        // For this purpose, we create a `SimpleProtocol` struct.
        .with_upgrade(SimpleProtocol::new("/echo/1.0.0", |socket| {
            // This closure is called whenever a stream using the "echo" protocol has been
            // successfully negotiated. The parameter is the raw socket (implements the AsyncRead
            // and AsyncWrite traits), and the closure must return an implementation of
            // `IntoFuture` that can yield any type of object.
            Ok(length_delimited::Framed::<_, BytesMut>::new(socket))
        }));

    // We now have a `transport` variable that can be used either to dial nodes or listen to
    // incoming connections, and that will automatically apply all the selected protocols on top
    // of any opened stream.

    // We use it to dial the address.
    let dialer = transport_with_echo
        .dial(swarm::Multiaddr::new(&target_addr).expect("invalid multiaddr"))
        // If the multiaddr protocol exists but is not supported, then we get an error containing
        // the transport and the original multiaddress. Therefore we cannot directly use `unwrap()`
        // or `expect()`, but have to add a `map_err()` beforehand.
        .map_err(|(_, addr)| addr).expect("unsupported multiaddr")

        .and_then(|echo| {
            // `echo` is what the closure used when initializing "echo" returns.
            // Consequently, please note that the `send` method is available only because the type
            // `length_delimited::Framed` has a `send` method.
            println!("Sending \"hello world\" to listener");
            echo.send("hello world".into())
        })
        .and_then(|echo| {
            // The message has been successfully sent. Now wait for an answer.
            echo.into_future()
                .map(|(msg, rest)| {
                    println!("Received message from listener: {:?}", msg);
                    rest
                })
                .map_err(|(err, _)| err)
        });

    // `dialer` is a future that contains all the behaviour that we want, but nothing has actually
    // started yet. Because we created the `TcpConfig` with tokio, we need to run the future
    // through the tokio core.
    core.run(dialer.map(|_| ()).select(transport.incoming().for_each(|_| Ok(()))))
        .unwrap_or_else(|_| panic!());
}
