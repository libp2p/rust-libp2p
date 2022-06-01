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

use futures::{future, prelude::*};
use libp2p_core::{transport::Transport, upgrade};
use libp2p_deflate::DeflateConfig;
use libp2p_tcp::TcpConfig;
use quickcheck::{QuickCheck, RngCore, TestResult};

#[test]
fn deflate() {
    fn prop(message: Vec<u8>) -> TestResult {
        if message.is_empty() {
            return TestResult::discard();
        }
        async_std::task::block_on(run(message));
        TestResult::passed()
    }
    QuickCheck::new().quickcheck(prop as fn(Vec<u8>) -> TestResult)
}

#[test]
fn lot_of_data() {
    let mut v = vec![0; 2 * 1024 * 1024];
    rand::thread_rng().fill_bytes(&mut v);
    async_std::task::block_on(run(v))
}

async fn run(message1: Vec<u8>) {
    let mut transport = TcpConfig::new().and_then(|conn, endpoint| {
        upgrade::apply(
            conn,
            DeflateConfig::default(),
            endpoint,
            upgrade::Version::V1,
        )
    });

    let mut listener = transport
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().expect("multiaddr"))
        .expect("listener");

    let listen_addr = listener
        .by_ref()
        .next()
        .await
        .expect("some event")
        .expect("no error")
        .into_new_address()
        .expect("new address");

    let message2 = message1.clone();

    let listener_task = async_std::task::spawn(async move {
        let mut conn = listener
            .filter(|e| future::ready(e.as_ref().map(|e| e.is_upgrade()).unwrap_or(false)))
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_upgrade()
            .expect("upgrade")
            .0
            .await
            .expect("connection");

        let mut buf = vec![0; message2.len()];
        conn.read_exact(&mut buf).await.expect("read_exact");
        assert_eq!(&buf[..], &message2[..]);

        conn.write_all(&message2).await.expect("write_all");
        conn.close().await.expect("close")
    });

    let mut conn = transport
        .dial(listen_addr)
        .expect("dialer")
        .await
        .expect("connection");
    conn.write_all(&message1).await.expect("write_all");
    conn.close().await.expect("close");

    let mut buf = Vec::new();
    conn.read_to_end(&mut buf).await.expect("read_to_end");
    assert_eq!(&buf[..], &message1[..]);

    listener_task.await
}
