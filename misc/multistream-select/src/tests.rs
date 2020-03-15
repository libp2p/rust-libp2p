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

        let server = async_std::task::spawn(async move {
            let connec = listener.accept().await.unwrap().0;
            let protos = vec![b"/proto1", b"/proto2"];
            let (proto, mut io) = listener_select_proto(connec, protos).await.unwrap();
            assert_eq!(proto, b"/proto2");

            let mut out = vec![0; 32];
            let n = io.read(&mut out).await.unwrap();
            out.truncate(n);
            assert_eq!(out, b"ping");

            io.write_all(b"pong").await.unwrap();
            io.flush().await.unwrap();
        });

        let client = async_std::task::spawn(async move {
            let connec = TcpStream::connect(&listener_addr).await.unwrap();
            let protos = vec![b"/proto3", b"/proto2"];
            let (proto, mut io) = dialer_select_proto(connec, protos.into_iter(), version)
                .await.unwrap();
            assert_eq!(proto, b"/proto2");

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

#[test]
fn no_protocol_found() {
    async fn run(version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = async_std::task::spawn(async move {
            let connec = listener.accept().await.unwrap().0;
            let protos = vec![b"/proto1", b"/proto2"];
            let io = match listener_select_proto(connec, protos).await {
                Ok((_, io)) => io,
                // We don't explicitly check for `Failed` because the client might close the connection when it
                // realizes that we have no protocol in common.
                Err(_) => return,
            };
            match io.complete().await {
                Err(NegotiationError::Failed) => {},
                _ => panic!(),
            }
        });

        let client = async_std::task::spawn(async move {
            let connec = TcpStream::connect(&listener_addr).await.unwrap();
            let protos = vec![b"/proto3", b"/proto4"];
            let io = match dialer_select_proto(connec, protos.into_iter(), version).await {
                Err(NegotiationError::Failed) => return,
                Ok((_, io)) => io,
                Err(_) => panic!()
            };
            match io.complete().await {
                Err(NegotiationError::Failed) => {},
                _ => panic!(),
            }
        });

        server.await;
        client.await;
    }

    async_std::task::block_on(run(Version::V1));
    async_std::task::block_on(run(Version::V1Lazy));
}

#[test]
fn select_proto_parallel() {
    async fn run(version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = async_std::task::spawn(async move {
            let connec = listener.accept().await.unwrap().0;
            let protos = vec![b"/proto1", b"/proto2"];
            let (proto, io) = listener_select_proto(connec, protos).await.unwrap();
            assert_eq!(proto, b"/proto2");
            io.complete().await.unwrap();
        });

        let client = async_std::task::spawn(async move {
            let connec = TcpStream::connect(&listener_addr).await.unwrap();
            let protos = vec![b"/proto3", b"/proto2"];
            let (proto, io) = dialer_select_proto_parallel(connec, protos.into_iter(), version)
                .await.unwrap();
            assert_eq!(proto, b"/proto2");
            io.complete().await.unwrap();
        });

        server.await;
        client.await;
    }

    async_std::task::block_on(run(Version::V1));
    async_std::task::block_on(run(Version::V1Lazy));
}

#[test]
fn select_proto_serial() {
    async fn run(version: Version) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = async_std::task::spawn(async move {
            let connec = listener.accept().await.unwrap().0;
            let protos = vec![b"/proto1", b"/proto2"];
            let (proto, io) = listener_select_proto(connec, protos).await.unwrap();
            assert_eq!(proto, b"/proto2");
            io.complete().await.unwrap();
        });

        let client = async_std::task::spawn(async move {
            let connec = TcpStream::connect(&listener_addr).await.unwrap();
            let protos = vec![b"/proto3", b"/proto2"];
            let (proto, io) = dialer_select_proto_serial(connec, protos.into_iter(), version)
                .await.unwrap();
            assert_eq!(proto, b"/proto2");
            io.complete().await.unwrap();
        });

        server.await;
        client.await;
    }

    async_std::task::block_on(run(Version::V1));
    async_std::task::block_on(run(Version::V1Lazy));
}
