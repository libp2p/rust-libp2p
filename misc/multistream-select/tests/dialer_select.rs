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

//! Integration tests for protocol negotiation.

use futures::prelude::*;
use multistream_select::{dialer_select_proto, listener_select_proto, NegotiationError, Version};
use std::time::Duration;

#[test]
fn select_proto_basic() {
    async fn run(version: Version) {
        let (client_connection, server_connection) = futures_ringbuf::Endpoint::pair(100, 100);

        let server = async_std::task::spawn(async move {
            let protos = vec!["/proto1", "/proto2"];
            let (proto, mut io) = listener_select_proto(server_connection, protos)
                .await
                .unwrap();
            assert_eq!(proto, "/proto2");

            let mut out = vec![0; 32];
            let n = io.read(&mut out).await.unwrap();
            out.truncate(n);
            assert_eq!(out, b"ping");

            io.write_all(b"pong").await.unwrap();
            io.flush().await.unwrap();
        });

        let client = async_std::task::spawn(async move {
            let protos = vec!["/proto3", "/proto2"];
            let (proto, mut io) = dialer_select_proto(client_connection, protos, version)
                .await
                .unwrap();
            assert_eq!(proto, "/proto2");

            io.write_all(b"ping").await.unwrap();
            io.flush().await.unwrap();

            let mut out = vec![0; 32];
            let n = io.read(&mut out).await.unwrap();
            out.truncate(n);
            assert_eq!(out, b"pong");
        });

        server.await;
        client.await;
    }

    async_std::task::block_on(run(Version::V1));
    async_std::task::block_on(run(Version::V1Lazy));
}

/// Tests the expected behaviour of failed negotiations.
#[test]
fn negotiation_failed() {
    let _ = env_logger::try_init();

    async fn run(
        Test {
            version,
            listen_protos,
            dial_protos,
            dial_payload,
        }: Test,
    ) {
        let (client_connection, server_connection) = futures_ringbuf::Endpoint::pair(100, 100);

        let server = async_std::task::spawn(async move {
            let io = match listener_select_proto(server_connection, listen_protos).await {
                Ok((_, io)) => io,
                Err(NegotiationError::Failed) => return,
                Err(NegotiationError::ProtocolError(e)) => {
                    panic!("Unexpected protocol error {e}")
                }
            };
            match io.complete().await {
                Err(NegotiationError::Failed) => {}
                _ => panic!(),
            }
        });

        let client = async_std::task::spawn(async move {
            let mut io = match dialer_select_proto(client_connection, dial_protos, version).await {
                Err(NegotiationError::Failed) => return,
                Ok((_, io)) => io,
                Err(_) => panic!(),
            };
            // The dialer may write a payload that is even sent before it
            // got confirmation of the last proposed protocol, when `V1Lazy`
            // is used.
            io.write_all(&dial_payload).await.unwrap();
            match io.complete().await {
                Err(NegotiationError::Failed) => {}
                _ => panic!(),
            }
        });

        server.await;
        client.await;
    }

    /// Parameters for a single test run.
    #[derive(Clone)]
    struct Test {
        version: Version,
        listen_protos: Vec<&'static str>,
        dial_protos: Vec<&'static str>,
        dial_payload: Vec<u8>,
    }

    // Disjunct combinations of listen and dial protocols to test.
    //
    // The choices here cover the main distinction between a single
    // and multiple protocols.
    let protos = vec![
        (vec!["/proto1"], vec!["/proto2"]),
        (vec!["/proto1", "/proto2"], vec!["/proto3", "/proto4"]),
    ];

    // The payloads that the dialer sends after "successful" negotiation,
    // which may be sent even before the dialer got protocol confirmation
    // when `V1Lazy` is used.
    //
    // The choices here cover the specific situations that can arise with
    // `V1Lazy` and which must nevertheless behave identically to `V1` w.r.t.
    // the outcome of the negotiation.
    let payloads = vec![
        // No payload, in which case all versions should behave identically
        // in any case, i.e. the baseline test.
        vec![],
        // With this payload and `V1Lazy`, the listener interprets the first
        // `1` as a message length and encounters an invalid message (the
        // second `1`). The listener is nevertheless expected to fail
        // negotiation normally, just like with `V1`.
        vec![1, 1],
        // With this payload and `V1Lazy`, the listener interprets the first
        // `42` as a message length and encounters unexpected EOF trying to
        // read a message of that length. The listener is nevertheless expected
        // to fail negotiation normally, just like with `V1`
        vec![42, 1],
    ];

    for (listen_protos, dial_protos) in protos {
        for dial_payload in payloads.clone() {
            for &version in &[Version::V1, Version::V1Lazy] {
                async_std::task::block_on(run(Test {
                    version,
                    listen_protos: listen_protos.clone(),
                    dial_protos: dial_protos.clone(),
                    dial_payload: dial_payload.clone(),
                }))
            }
        }
    }
}

#[async_std::test]
async fn v1_lazy_do_not_wait_for_negotiation_on_poll_close() {
    let (client_connection, _server_connection) = futures_ringbuf::Endpoint::pair(1024 * 1024, 1);

    let client = async_std::task::spawn(async move {
        // Single protocol to allow for lazy (or optimistic) protocol negotiation.
        let protos = vec!["/proto1"];
        let (proto, mut io) = dialer_select_proto(client_connection, protos, Version::V1Lazy)
            .await
            .unwrap();
        assert_eq!(proto, "/proto1");

        // client can close the connection even though protocol negotiation is not yet done, i.e.
        // `_server_connection` had been untouched.
        io.close().await.unwrap();
    });

    async_std::future::timeout(Duration::from_secs(10), client)
        .await
        .unwrap();
}
