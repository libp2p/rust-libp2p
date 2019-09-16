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

use futures::{prelude::*, channel::oneshot};
use libp2p_core::{transport::Transport, upgrade};
use libp2p_deflate::DeflateConfig;
use libp2p_tcp::TcpConfig;
use quickcheck::QuickCheck;

#[test]
fn deflate() {
    fn prop(message: Vec<u8>) -> bool {
        run(message);
        true
    }

    QuickCheck::new()
        .max_tests(30)
        .quickcheck(prop as fn(Vec<u8>) -> bool)
}

#[test]
fn lot_of_data() {
    run((0..16*1024*1024).map(|_| rand::random::<u8>()).collect());
}

fn run(message1: Vec<u8>) {
    let transport1 = TcpConfig::new().and_then(|c, e| upgrade::apply(c, DeflateConfig::default(), e));
    let transport2 = transport1.clone();
    let message2 = message1.clone();
    let (l_a_tx, l_a_rx) = oneshot::channel();

    async_std::task::spawn(async move {
        let mut server = transport1.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
        let server_address = server.next().await.unwrap().unwrap().into_new_address().unwrap();
        l_a_tx.send(server_address).unwrap();

        let mut connec = server.next().await.unwrap().unwrap().into_upgrade().unwrap().0.await.unwrap();

        let mut buf = vec![0; message2.len()];
        connec.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf[..], &message2[..]);

        connec.write_all(&message2).await.unwrap();
        connec.close().await.unwrap();
    });

    futures::executor::block_on(async move {
        let listen_addr = l_a_rx.await.unwrap();
        let mut connec = transport2.dial(listen_addr).unwrap().await.unwrap();
        connec.write_all(&message1).await.unwrap();
        connec.close().await.unwrap();

        let mut buf = Vec::new();
        connec.read_to_end(&mut buf).await.unwrap();
        assert_eq!(&buf[..], &message1[..]);
    });
}
