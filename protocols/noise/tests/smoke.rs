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
use libp2p_core::Transport;
use log::info;
use tokio::{self, io};

#[test]
fn smoke() {
    env_logger::init();

    let message = b"Lorem ipsum dolor sit amet...";

    let server_keypair = libp2p_noise::Keypair::gen_curve25519();
    let server_transport = libp2p_tcp::TcpConfig::new()
        .with_upgrade(libp2p_noise::NoiseConfig::xx(server_keypair));

    let (server, server_address) = server_transport
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();

    let server = server.take(1)
        .and_then(|client| client.0)
        .map_err(|e| panic!("server error: {}", e))
        .and_then(|(_, client)| {
            info!("server: reading message");
            io::read_to_end(client, Vec::new())
        })
        .for_each(move |msg| {
            info!("server: read message \"{}\"", std::str::from_utf8(&msg.1).unwrap());
            assert_eq!(msg.1, message);
            Ok(())
        });

    let client_keypair = libp2p_noise::Keypair::gen_curve25519();
    let client_transport = libp2p_tcp::TcpConfig::new()
        .with_upgrade(libp2p_noise::NoiseConfig::xx(client_keypair));

    let client = client_transport.dial(server_address).unwrap()
        .map_err(|e| panic!("client error: {}", e))
        .and_then(move |(_, server)| {
            info!("client: writing \"{}\"", std::str::from_utf8(message).unwrap());
            io::write_all(server, message).and_then(|(client, _)| io::flush(client))
        })
        .map(|_| ());

    let future = client.join(server)
        .map_err(|e| panic!("{:?}", e))
        .map(|_| ());

    tokio::run(future)
}

