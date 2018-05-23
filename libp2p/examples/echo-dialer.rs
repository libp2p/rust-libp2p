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
extern crate env_logger;
extern crate futures;
extern crate libp2p;
extern crate tokio_core;
extern crate tokio_io;

use futures::sync::oneshot;
use futures::{Future, Sink, Stream};
use std::env;
use libp2p::SimpleProtocol;
use libp2p::core::Transport;
use libp2p::core::upgrade;
use libp2p::tcp::TcpConfig;
use tokio_core::reactor::Core;
use tokio_io::AsyncRead;
use tokio_io::codec::BytesCodec;
use libp2p::websocket::WsConfig;

fn main() {
    env_logger::init();

    // Determine which address to dial.
    let target_addr = env::args()
        .nth(1)
        .unwrap_or("/ip4/127.0.0.1/tcp/10333".to_owned());

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
            let plain_text = upgrade::PlainTextConfig;

            let secio = {
                let private_key = include_bytes!("test-private-key.pk8");
                let public_key = include_bytes!("test-public-key.der").to_vec();
                libp2p::secio::SecioConfig {
                    key: libp2p::secio::SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
                }
            };

            upgrade::or(plain_text, upgrade::map(secio, |(socket, _)| socket))
        })

        // On top of plaintext or secio, we will use the multiplex protocol.
        .with_upgrade(libp2p::mplex::MultiplexConfig::new())
        // The object returned by the call to `with_upgrade(MultiplexConfig::new())` can't be used as a
        // `Transport` because the output of the upgrade is not a stream but a controller for
        // muxing. We have to explicitly call `into_connection_reuse()` in order to turn this into
        // a `Transport`.
        .into_connection_reuse();

    // Building a struct that represents the protocol that we are going to use for dialing.
    let proto = SimpleProtocol::new("/echo/1.0.0", |socket| {
        // This closure is called whenever a stream using the "echo" protocol has been
        // successfully negotiated. The parameter is the raw socket (implements the AsyncRead
        // and AsyncWrite traits), and the closure must return an implementation of
        // `IntoFuture` that can yield any type of object.
        Ok(AsyncRead::framed(socket, BytesCodec::new()))
    });

    let (finished_tx, finished_rx) = oneshot::channel();
    let mut finished_tx = Some(finished_tx);

    // Let's put this `transport` into a *swarm*. The swarm will handle all the incoming
    // connections for us. The second parameter we pass is the connection upgrade that is accepted
    // by the listening part. We don't want to accept anything, so we pass a dummy object that
    // represents a connection that is always denied.
    let (swarm_controller, swarm_future) = libp2p::core::swarm(
        transport.clone().with_upgrade(proto.clone()),
        |echo, _client_addr| {
            // `echo` is what the closure used when initializing `proto` returns.
            // Consequently, please note that the `send` method is available only because the type
            // `length_delimited::Framed` has a `send` method.
            println!("Sending \"hello world\" to listener");
            let finished_tx = finished_tx.take();
            echo.send("hello world".into())
                // Then listening for one message from the remote.
                .and_then(|echo| {
                    echo.into_future().map_err(|(e, _)| e).map(|(n,_ )| n)
                })
                .and_then(move |message| {
                    println!("Received message from listener: {:?}", message.unwrap());
                    if let Some(finished_tx) = finished_tx {
                        finished_tx.send(()).unwrap();
                    }
                    Ok(())
                })
        },
    );

    // We now use the controller to dial to the address.
    swarm_controller
        .dial(target_addr.parse().expect("invalid multiaddr"), transport.with_upgrade(proto))
        // If the multiaddr protocol exists but is not supported, then we get an error containing
        // the original multiaddress.
        .expect("unsupported multiaddr");

    // The address we actually listen on can be different from the address that was passed to
    // the `listen_on` function. For example if you pass `/ip4/0.0.0.0/tcp/0`, then the port `0`
    // will be replaced with the actual port.

    // `swarm_future` is a future that contains all the behaviour that we want, but nothing has
    // actually started yet. Because we created the `TcpConfig` with tokio, we need to run the
    // future through the tokio core.
    let final_future = swarm_future
        .select(finished_rx.map_err(|_| unreachable!()))
        .map(|_| ())
        .map_err(|(err, _)| err);
    core.run(final_future).unwrap();
}
