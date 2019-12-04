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

//! Contains the unit tests of the library.

#![cfg(test)]

use crate::{Version, NegotiationError};
use crate::dialer_select::{dialer_select_proto_parallel, dialer_select_proto_serial};
use crate::{dialer_select_proto, listener_select_proto};

use async_std::net::{TcpListener, TcpStream};
use futures::prelude::*;

#[test]
fn select_proto_basic() {
    async fn run(version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        async_std::task::spawn(async move {
            let connec = listener
                .accept()
                .await
                .unwrap()
                .0;
            let protos = vec![b"/proto1", b"/proto2"];
            let (proto, mut io) = listener_select_proto(connec, protos).await.unwrap();
            assert_eq!(proto, b"/proto2");
            io.write_all(b"pong").await.unwrap();
            io.flush().await.unwrap();
        });

        let connec = TcpStream::connect(&listener_addr).await.unwrap();
        let protos = vec![b"/proto3", b"/proto2"];
        let (proto, mut io) = dialer_select_proto(connec, protos, version).await.unwrap();
        assert_eq!(proto, b"/proto2");
        io.write_all(b"ping").await.unwrap();
        io.flush().await.unwrap();

        let mut buf = [0u8; 4];
        io.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    async_std::task::block_on(async move {
        run(Version::V1).await;
        run(Version::V1Lazy).await;
    })
}

#[test]
fn no_protocol_found() {
    async fn run(version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        async_std::task::spawn(async move {
            let connec = listener
                .accept()
                .await
                .unwrap()
                .0;
            let protos = vec![b"/proto1", b"/proto2"];
            let _ = listener_select_proto(connec, protos).await;
        });

        let connec = TcpStream::connect(&listener_addr).await.unwrap();
        let protos = vec![b"/proto3", b"/proto4"];
        match dialer_select_proto(connec, protos, version).await {
            Ok((_, io)) => assert!(io.complete().await.is_err()),
            Err(NegotiationError::Failed) => {},
            _ => panic!()
        };
    }

    async_std::task::block_on(async move {
        run(Version::V1).await;
        run(Version::V1Lazy).await;
    })
}

#[test]
fn select_proto_parallel() {
    async fn run(version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        async_std::task::spawn(async move {
            let connec = listener
                .accept()
                .await
                .unwrap()
                .0;
            let protos = vec![b"/proto1", b"/proto2"];
            let (proto, mut io) = listener_select_proto(connec, protos).await.unwrap();
            assert_eq!(proto, b"/proto2");
            io.write_all(b"pong").await.unwrap();
            io.flush().await.unwrap();
        });

        let connec = TcpStream::connect(&listener_addr).await.unwrap();
        let protos = vec![b"/proto3", b"/proto2"];
        let (proto, mut io) = dialer_select_proto_parallel(connec, protos, version).await.unwrap();
        assert_eq!(proto, b"/proto2");
        io.write_all(b"ping").await.unwrap();
        io.flush().await.unwrap();

        let mut buf = [0u8; 4];
        io.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    async_std::task::block_on(async move {
        run(Version::V1).await;
        run(Version::V1Lazy).await;
    })
}

#[test]
fn select_proto_serial() {
    async fn run(version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        async_std::task::spawn(async move {
            let connec = listener
                .accept()
                .await
                .unwrap()
                .0;
            let protos = vec![b"/proto1", b"/proto2"];
            let (proto, mut io) = listener_select_proto(connec, protos).await.unwrap();
            assert_eq!(proto, b"/proto2");
            io.write_all(b"pong").await.unwrap();
            io.flush().await.unwrap();
        });

        let connec = TcpStream::connect(&listener_addr).await.unwrap();
        let protos = vec![b"/proto3", b"/proto2"];
        let (proto, mut io) = dialer_select_proto_serial(connec, protos, version).await.unwrap();
        assert_eq!(proto, b"/proto2");
        io.write_all(b"ping").await.unwrap();
        io.flush().await.unwrap();

        let mut buf = [0u8; 4];
        io.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    async_std::task::block_on(async move {
        run(Version::V1).await;
        run(Version::V1Lazy).await;
    })
}
