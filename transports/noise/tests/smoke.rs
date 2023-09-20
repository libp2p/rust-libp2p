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
use libp2p_core::transport::{MemoryTransport, Transport};
use libp2p_core::{upgrade, InboundUpgrade, OutboundUpgrade};
use libp2p_identity as identity;
use libp2p_noise as noise;
use log::info;
use quickcheck::*;
use std::{convert::TryInto, io};

#[allow(dead_code)]
fn core_upgrade_compat() {
    // Tests API compaibility with the libp2p-core upgrade API,
    // i.e. if it compiles, the "test" is considered a success.
    let id_keys = identity::Keypair::generate_ed25519();
    let noise = noise::Config::new(&id_keys).unwrap();
    let _ = MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(noise);
}

#[test]
fn xx() {
    let _ = env_logger::try_init();
    fn prop(mut messages: Vec<Message>) -> bool {
        messages.truncate(5);
        let server_id = identity::Keypair::generate_ed25519();
        let client_id = identity::Keypair::generate_ed25519();

        let (client, server) = futures_ringbuf::Endpoint::pair(100, 100);

        futures::executor::block_on(async move {
            let (
                (reported_client_id, mut server_session),
                (reported_server_id, mut client_session),
            ) = futures::future::try_join(
                noise::Config::new(&server_id)
                    .unwrap()
                    .upgrade_inbound(server, ""),
                noise::Config::new(&client_id)
                    .unwrap()
                    .upgrade_outbound(client, ""),
            )
            .await
            .unwrap();

            assert_eq!(reported_client_id, client_id.public().to_peer_id());
            assert_eq!(reported_server_id, server_id.public().to_peer_id());

            let client_fut = async {
                for m in &messages {
                    let n = (m.0.len() as u64).to_be_bytes();
                    client_session.write_all(&n[..]).await.expect("len written");
                    client_session.write_all(&m.0).await.expect("no error")
                }
                client_session.flush().await.expect("no error");
            };

            let server_fut = async {
                for m in &messages {
                    let len = {
                        let mut n = [0; 8];
                        match server_session.read_exact(&mut n).await {
                            Ok(()) => u64::from_be_bytes(n),
                            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => 0,
                            Err(e) => panic!("error reading len: {e}"),
                        }
                    };
                    info!("server: reading message ({} bytes)", len);
                    let mut server_buffer = vec![0; len.try_into().unwrap()];
                    server_session
                        .read_exact(&mut server_buffer)
                        .await
                        .expect("no error");
                    assert_eq!(server_buffer, m.0)
                }
            };

            futures::future::join(client_fut, server_fut).await;
        });

        true
    }
    QuickCheck::new()
        .max_tests(30)
        .quickcheck(prop as fn(Vec<Message>) -> bool)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Message(Vec<u8>);

impl Arbitrary for Message {
    fn arbitrary(g: &mut Gen) -> Self {
        let s = g.gen_range(1..128 * 1024);
        let mut v = vec![0; s];
        for b in &mut v {
            *b = u8::arbitrary(g);
        }
        Message(v)
    }
}
