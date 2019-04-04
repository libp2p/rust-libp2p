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

use futures::{future::Either, prelude::*};
use libp2p_core::{Transport, upgrade::{apply_inbound, apply_outbound}};
use libp2p_noise::{Keypair, X25519, NoiseConfig};
use libp2p_tcp::TcpConfig;
use log::info;
use quickcheck::QuickCheck;
use tokio::{self, io};

#[test]
fn xx() {
    let _ = env_logger::try_init();
    fn prop(message: Vec<u8>) -> bool {

        let server_keypair = Keypair::<X25519>::new();
        let server_transport = TcpConfig::new().with_upgrade(NoiseConfig::xx(server_keypair));

        let client_keypair = Keypair::<X25519>::new();
        let client_transport = TcpConfig::new().with_upgrade(NoiseConfig::xx(client_keypair));

        run(server_transport, client_transport, message);
        true
    }
    QuickCheck::new().max_tests(30).quickcheck(prop as fn(Vec<u8>) -> bool)
}

#[test]
fn ix() {
    let _ = env_logger::try_init();
    fn prop(message: Vec<u8>) -> bool {

        let server_keypair = Keypair::<X25519>::new();
        let server_transport = TcpConfig::new().with_upgrade(NoiseConfig::ix(server_keypair));

        let client_keypair = Keypair::<X25519>::new();
        let client_transport = TcpConfig::new().with_upgrade(NoiseConfig::ix(client_keypair));

        run(server_transport, client_transport, message);
        true
    }
    QuickCheck::new().max_tests(30).quickcheck(prop as fn(Vec<u8>) -> bool)
}

#[test]
fn ik_xx() {
    let _ = env_logger::try_init();
    fn prop(message: Vec<u8>) -> bool {
        let server_keypair = Keypair::<X25519>::new();
        let server_public = server_keypair.public().clone();
        let server_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                if endpoint.is_listener() {
                    Either::A(apply_inbound(output, NoiseConfig::ik_listener(server_keypair)))
                } else {
                    Either::B(apply_outbound(output, NoiseConfig::xx(server_keypair)))
                }
            });

        let client_keypair = Keypair::<X25519>::new();
        let client_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                if endpoint.is_dialer() {
                    Either::A(apply_outbound(output, NoiseConfig::ik_dialer(client_keypair, server_public)))
                } else {
                    Either::B(apply_inbound(output, NoiseConfig::xx(client_keypair)))
                }
            });

        run(server_transport, client_transport, message);
        true
    }
    QuickCheck::new().max_tests(30).quickcheck(prop as fn(Vec<u8>) -> bool)
}

fn run<T, A, U, B, P>(server_transport: T, client_transport: U, message1: Vec<u8>)
where
    T: Transport<Output = (P, A)>,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    A: io::AsyncRead + io::AsyncWrite + Send + 'static,
    U: Transport<Output = (P, B)>,
    U::Dial: Send + 'static,
    U::Listener: Send + 'static,
    U::ListenerUpgrade: Send + 'static,
    B: io::AsyncRead + io::AsyncWrite + Send + 'static
{
    let message2 = message1.clone();

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
            assert_eq!(msg.1, message1);
            Ok(())
        });

    let client = client_transport.dial(server_address).unwrap()
        .map_err(|e| panic!("client error: {}", e))
        .and_then(move |(_, server)| {
            io::write_all(server, message2).and_then(|(client, _)| io::flush(client))
        })
        .map(|_| ());

    let future = client.join(server)
        .map_err(|e| panic!("{:?}", e))
        .map(|_| ());

    tokio::run(future)
}

