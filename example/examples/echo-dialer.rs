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
use swarm::{SimpleProtocol, Transport};
use tcp::TcpConfig;
use tokio_core::reactor::Core;
use tokio_io::codec::length_delimited;

fn main() {
    // We start by building the tokio engine that will run all the sockets.
    let mut core = Core::new().unwrap();

    // Now let's build the transport stack.
    // We start by creating a `TcpConfig` that indicates that we want TCP/IP.
    let transport = TcpConfig::new(core.handle())

        // On top of TCP/IP, we will use either the plaintext protocol or the secio protocol,
        // depending on which one the remote supports.
        .with_upgrade(swarm::PlainTextConfig)
        .or_upgrade({
            let private_key = include_bytes!("test-private-key.pk8");
            let public_key = include_bytes!("test-public-key.der").to_vec();
            secio::SecioConfig {
                key: secio::SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
            }
        })

        .with_upgrade(multiplex::MultiplexConfig);
    let transport: swarm::ConnectionReuse<_, _> = transport.into();

    let transport = transport
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

    // We use it to dial `/ip4/127.0.0.1/tcp/10333`.
    let dialer = transport
        .dial_and_listen(swarm::Multiaddr::new("/ip4/127.0.0.1/tcp/10333").unwrap())
        .unwrap_or_else(|_| panic!("unsupported multiaddr protocol ; should never happen"))
        .and_then(|(incoming, echo)| {
            // `echo` is what the closure used when initializing "echo" returns.
            // Consequently, please note that the `send` method is available only because the type
            // `length_delimited::Framed` has a `send` method.
            echo.and_then(|echo| echo.send("hello world".into()).map(Option::Some))
                .select(
                    incoming
                        .for_each(|_| {
                            println!("opened");
                            Ok(())
                        })
                        .map(|()| None),
                )
                .map(|(n, _)| n)
                .map_err(|(e, _)| e)
        })
        .and_then(|echo| {
            // The message has been successfully sent. Now wait for an answer.
            echo.unwrap()
                .into_future()
                .map(|(msg, rest)| {
                    println!("received: {:?}", msg);
                    rest
                })
                .map_err(|(err, _)| err)
        });

    // `dialer` is a future that contains all the behaviour that we want, but nothing has actually
    // started yet. Because we created the `TcpConfig` with tokio, we need to run the future
    // through the tokio core.
    core.run(dialer).unwrap();
}
