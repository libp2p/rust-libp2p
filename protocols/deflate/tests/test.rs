// Copyright 2019 Parity Technologies (UK) Ltd.
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

use futures::prelude::*;
use libp2p_core::transport::{ListenerEvent, Transport};
use libp2p_core::upgrade::{self, Negotiated};
use libp2p_deflate::{DeflateConfig, DeflateOutput};
use libp2p_tcp::{TcpConfig, TcpTransStream};
use log::info;
use quickcheck::QuickCheck;
use tokio::{self, io};

#[test]
fn deflate() {
    let _ = env_logger::try_init();

    fn prop(message: Vec<u8>) -> bool {
        let client = TcpConfig::new().and_then(|c, e|
            upgrade::apply(c, DeflateConfig {}, e, upgrade::Version::V1));
        let server = client.clone();
        run(server, client, message);
        true
    }

    QuickCheck::new()
        .max_tests(30)
        .quickcheck(prop as fn(Vec<u8>) -> bool)
}

type Output = DeflateOutput<Negotiated<TcpTransStream>>;

fn run<T>(server_transport: T, client_transport: T, message1: Vec<u8>)
where
    T: Transport<Output = Output>,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
{
    let message2 = message1.clone();

    let mut server = server_transport
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();
    let server_address = server
        .by_ref()
        .wait()
        .next()
        .expect("some event")
        .expect("no error")
        .into_new_address()
        .expect("listen address");
    let server = server
        .take(1)
        .filter_map(ListenerEvent::into_upgrade)
        .and_then(|(client, _)| client)
        .map_err(|e| panic!("server error: {}", e))
        .and_then(|client| {
            info!("server: reading message");
            io::read_to_end(client, Vec::new())
        })
        .for_each(move |(_, msg)| {
            info!("server: read message: {:?}", msg);
            assert_eq!(msg, message1);
            Ok(())
        });

    let client = client_transport
        .dial(server_address.clone())
        .unwrap()
        .map_err(|e| panic!("client error: {}", e))
        .and_then(move |server| {
            io::write_all(server, message2).and_then(|(client, _)| io::shutdown(client))
        })
        .map(|_| ());

    let future = client
        .join(server)
        .map_err(|e| panic!("{:?}", e))
        .map(|_| ());

    tokio::run(future)
}
