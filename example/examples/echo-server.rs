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
extern crate libp2p_websocket as websocket;
extern crate multiplex;
extern crate tokio_core;
extern crate tokio_io;

use futures::future::{loop_fn, Future, IntoFuture, Loop};
use futures::{Sink, Stream};
use std::env;
use swarm::{SimpleProtocol, Transport, UpgradeExt};
use tcp::TcpConfig;
use tokio_core::reactor::Core;
use tokio_io::AsyncRead;
use tokio_io::codec::BytesCodec;
use websocket::WsConfig;

fn main() {
    // Determine which address to listen to.
    let listen_addr = env::args()
        .nth(1)
        .unwrap_or("/ip4/0.0.0.0/tcp/10333".to_owned());

    // We start by building the tokio engine that will run all the sockets.
    let mut core = Core::new().unwrap();

    // Now let's build the transport stack.
    // We start by creating a `TcpConfig` that indicates that we want TCP/IP.
    let transport = TcpConfig::new(core.handle())
        // In addition to TCP/IP, we also want to support the Websockets protocol on top of TCP/IP.
        // The parameter passed to `WsConfig::new()` must be an implementation of `Transport` to be
        // used for the underlying multiaddress.
        .or_transport(WsConfig::new(TcpConfig::new(core.handle())))

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

    // We now have a `transport` variable that can be used either to dial nodes or listen to
    // incoming connections, and that will automatically apply secio and multiplex on top
    // of any opened stream.

    // We now prepare the protocol that we are going to negotiate with nodes that open a connection
    // or substream to our server.
    let proto = SimpleProtocol::new("/echo/1.0.0", |socket| {
        // This closure is called whenever a stream using the "echo" protocol has been
        // successfully negotiated. The parameter is the raw socket (implements the AsyncRead
        // and AsyncWrite traits), and the closure must return an implementation of
        // `IntoFuture` that can yield any type of object.
        Ok(AsyncRead::framed(socket, BytesCodec::new()))
    });

    // Let's put this `transport` into a *swarm*. The swarm will handle all the incoming and
    // outgoing connections for us.
    let (swarm_controller, swarm_future) = swarm::swarm(transport, proto, |socket, client_addr| {
        println!("Successfully negotiated protocol with {}", client_addr);

        // The type of `socket` is exactly what the closure of `SimpleProtocol` returns.

        // We loop forever in order to handle all the messages sent by the client.
        loop_fn(socket, move |socket| {
            let client_addr = client_addr.clone();
            socket
                .into_future()
                .map_err(|(e, _)| e)
                .and_then(move |(msg, rest)| {
                    if let Some(msg) = msg {
                        // One message has been received. We send it back to the client.
                        println!(
                            "Received a message from {}: {:?}\n => Sending back \
                             identical message to remote",
                            client_addr, msg
                        );
                        Box::new(rest.send(msg.freeze()).map(|m| Loop::Continue(m)))
                            as Box<Future<Item = _, Error = _>>
                    } else {
                        // End of stream. Connection closed. Breaking the loop.
                        println!("Received EOF from {}\n => Dropping connection", client_addr);
                        Box::new(Ok(Loop::Break(())).into_future())
                            as Box<Future<Item = _, Error = _>>
                    }
                })
        })
    });

    // We now use the controller to listen on the address.
    let address = swarm_controller
        .listen_on(listen_addr.parse().expect("invalid multiaddr"))
        // If the multiaddr protocol exists but is not supported, then we get an error containing
        // the original multiaddress.
        .expect("unsupported multiaddr");
    // The address we actually listen on can be different from the address that was passed to
    // the `listen_on` function. For example if you pass `/ip4/0.0.0.0/tcp/0`, then the port `0`
    // will be replaced with the actual port.
    println!("Now listening on {:?}", address);

    // `swarm_future` is a future that contains all the behaviour that we want, but nothing has
    // actually started yet. Because we created the `TcpConfig` with tokio, we need to run the
    // future through the tokio core.
    core.run(swarm_future).unwrap();
}
