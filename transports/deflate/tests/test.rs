#![allow(deprecated)]

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
use libp2p_core::OutboundUpgrade;
use libp2p_deflate::DeflateConfig;
use quickcheck::{QuickCheck, TestResult};
use rand::RngCore;

#[test]
fn deflate() {
    fn prop(message: Vec<u8>) -> TestResult {
        if message.is_empty() {
            return TestResult::discard();
        }
        futures::executor::block_on(run(message));
        TestResult::passed()
    }
    QuickCheck::new().quickcheck(prop as fn(Vec<u8>) -> TestResult)
}

#[test]
fn lot_of_data() {
    let mut v = vec![0; 2 * 1024 * 1024];
    rand::thread_rng().fill_bytes(&mut v);
    futures::executor::block_on(run(v));
}

async fn run(message1: Vec<u8>) {
    let (server, client) = futures_ringbuf::Endpoint::pair(100, 100);

    let message2 = message1.clone();

    let client_task = async move {
        let mut client = DeflateConfig::default()
            .upgrade_outbound(client, "")
            .await
            .unwrap();

        let mut buf = vec![0; message2.len()];
        client.read_exact(&mut buf).await.expect("read_exact");
        assert_eq!(&buf[..], &message2[..]);

        client.write_all(&message2).await.expect("write_all");
        client.close().await.expect("close")
    };

    let server_task = async move {
        let mut server = DeflateConfig::default()
            .upgrade_outbound(server, "")
            .await
            .unwrap();

        server.write_all(&message1).await.expect("write_all");
        server.close().await.expect("close");

        let mut buf = Vec::new();
        server.read_to_end(&mut buf).await.expect("read_to_end");
        assert_eq!(&buf[..], &message1[..]);
    };

    futures::future::join(server_task, client_task).await;
}
