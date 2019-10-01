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

use futures::{future::{self, Either}, prelude::*};
use libp2p_core::identity;
use libp2p_core::upgrade::{self, Negotiated, apply_inbound, apply_outbound};
use libp2p_core::transport::{Transport, ListenerEvent};
use libp2p_noise::{Keypair, X25519, NoiseConfig, RemoteIdentity, NoiseError, NoiseOutput};
use libp2p_tcp::{TcpConfig, TcpTransStream};
use log::info;
use quickcheck::QuickCheck;
use tokio::{self, io};

#[allow(dead_code)]
fn core_upgrade_compat() {
    // Tests API compaibility with the libp2p-core upgrade API,
    // i.e. if it compiles, the "test" is considered a success.
    let id_keys = identity::Keypair::generate_ed25519();
    let dh_keys = Keypair::<X25519>::new().into_authentic(&id_keys).unwrap();
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();
    let _ = TcpConfig::new().upgrade(upgrade::Version::V1).authenticate(noise);
}

#[test]
fn xx() {
    let _ = env_logger::try_init();
    fn prop(message: Vec<u8>) -> bool {
        let server_id = identity::Keypair::generate_ed25519();
        let client_id = identity::Keypair::generate_ed25519();

        let server_id_public = server_id.public();
        let client_id_public = client_id.public();

        let server_dh = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();
        let server_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                upgrade::apply(output, NoiseConfig::xx(server_dh), endpoint, upgrade::Version::V1)
            })
            .and_then(move |out, _| expect_identity(out, &client_id_public));

        let client_dh = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();
        let client_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                upgrade::apply(output, NoiseConfig::xx(client_dh), endpoint, upgrade::Version::V1)
            })
            .and_then(move |out, _| expect_identity(out, &server_id_public));

        run(server_transport, client_transport, message);
        true
    }
    QuickCheck::new().max_tests(30).quickcheck(prop as fn(Vec<u8>) -> bool)
}

#[test]
fn ix() {
    let _ = env_logger::try_init();
    fn prop(message: Vec<u8>) -> bool {
        let server_id = identity::Keypair::generate_ed25519();
        let client_id = identity::Keypair::generate_ed25519();

        let server_id_public = server_id.public();
        let client_id_public = client_id.public();

        let server_dh = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();
        let server_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                upgrade::apply(output, NoiseConfig::ix(server_dh), endpoint, upgrade::Version::V1)
            })
            .and_then(move |out, _| expect_identity(out, &client_id_public));

        let client_dh = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();
        let client_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                upgrade::apply(output, NoiseConfig::ix(client_dh), endpoint, upgrade::Version::V1)
            })
            .and_then(move |out, _| expect_identity(out, &server_id_public));

        run(server_transport, client_transport, message);
        true
    }
    QuickCheck::new().max_tests(30).quickcheck(prop as fn(Vec<u8>) -> bool)
}

#[test]
fn ik_xx() {
    let _ = env_logger::try_init();
    fn prop(message: Vec<u8>) -> bool {
        let server_id = identity::Keypair::generate_ed25519();
        let server_id_public = server_id.public();

        let client_id = identity::Keypair::generate_ed25519();
        let client_id_public = client_id.public();

        let server_dh = Keypair::<X25519>::new().into_authentic(&server_id).unwrap();
        let server_dh_public = server_dh.public().clone();
        let server_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                if endpoint.is_listener() {
                    Either::A(apply_inbound(output, NoiseConfig::ik_listener(server_dh)))
                } else {
                    Either::B(apply_outbound(output, NoiseConfig::xx(server_dh),
                        upgrade::Version::V1))
                }
            })
            .and_then(move |out, _| expect_identity(out, &client_id_public));

        let client_dh = Keypair::<X25519>::new().into_authentic(&client_id).unwrap();
        let server_id_public2 = server_id_public.clone();
        let client_transport = TcpConfig::new()
            .and_then(move |output, endpoint| {
                if endpoint.is_dialer() {
                    Either::A(apply_outbound(output,
                        NoiseConfig::ik_dialer(client_dh, server_id_public, server_dh_public),
                        upgrade::Version::V1))
                } else {
                    Either::B(apply_inbound(output, NoiseConfig::xx(client_dh)))
                }
            })
            .and_then(move |out, _| expect_identity(out, &server_id_public2));

        run(server_transport, client_transport, message);
        true
    }
    QuickCheck::new().max_tests(30).quickcheck(prop as fn(Vec<u8>) -> bool)
}

type Output = (RemoteIdentity<X25519>, NoiseOutput<Negotiated<TcpTransStream>>);

fn run<T, U>(server_transport: T, client_transport: U, message1: Vec<u8>)
where
    T: Transport<Output = Output>,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    U: Transport<Output = Output>,
    U::Dial: Send + 'static,
    U::Listener: Send + 'static,
    U::ListenerUpgrade: Send + 'static,
{
    let message2 = message1.clone();

    let mut server = server_transport
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();

    let server_address = server.by_ref().wait()
        .next()
        .expect("some event")
        .expect("no error")
        .into_new_address()
        .expect("listen address");

    let server = server.take(1)
        .filter_map(ListenerEvent::into_upgrade)
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

    let client = client_transport.dial(server_address.clone()).unwrap()
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

fn expect_identity(output: Output, pk: &identity::PublicKey)
    -> impl Future<Item = Output, Error = NoiseError>
{
    match output.0 {
        RemoteIdentity::IdentityKey(ref k) if k == pk => future::ok(output),
        _ => panic!("Unexpected remote identity")
    }
}
