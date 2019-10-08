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
use futures::prelude::*;
use tokio::runtime::current_thread::Runtime;
use tokio_tcp::{TcpListener, TcpStream};
use tokio_io::io as nio;

#[test]
fn select_proto_basic() {
    fn run(version: Version) {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map(|s| s.0.unwrap())
            .map_err(|(e, _)| e.into())
            .and_then(move |connec| {
                let protos = vec![b"/proto1", b"/proto2"];
                listener_select_proto(connec, protos)
            })
            .and_then(|(proto, io)| {
                nio::write_all(io, b"pong").from_err().map(move |_| proto)
            });

        let client = TcpStream::connect(&listener_addr)
            .from_err()
            .and_then(move |connec| {
                let protos = vec![b"/proto3", b"/proto2"];
                dialer_select_proto(connec, protos, version)
            })
            .and_then(|(proto, io)| {
                nio::write_all(io, b"ping").from_err().map(move |(io, _)| (proto, io))
            })
            .and_then(|(proto, io)| {
                nio::read_exact(io, [0; 4]).from_err().map(move |(_, msg)| {
                    assert_eq!(&msg, b"pong");
                    proto
                })
            });

        let mut rt = Runtime::new().unwrap();
        let (dialer_chosen, listener_chosen) =
            rt.block_on(client.join(server)).unwrap();

        assert_eq!(dialer_chosen, b"/proto2");
        assert_eq!(listener_chosen, b"/proto2");
    }

    run(Version::V1);
    run(Version::V1Lazy);
}

#[test]
fn no_protocol_found() {
    fn run(version: Version) {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map(|s| s.0.unwrap())
            .map_err(|(e, _)| e.into())
            .and_then(move |connec| {
                let protos = vec![b"/proto1", b"/proto2"];
                listener_select_proto(connec, protos)
            })
            .and_then(|(proto, io)| io.complete().map(move |_| proto));

        let client = TcpStream::connect(&listener_addr)
            .from_err()
            .and_then(move |connec| {
                let protos = vec![b"/proto3", b"/proto4"];
                dialer_select_proto(connec, protos, version)
            })
            .and_then(|(proto, io)| io.complete().map(move |_| proto));

        let mut rt = Runtime::new().unwrap();
        match rt.block_on(client.join(server)) {
            Err(NegotiationError::Failed) => (),
            e => panic!("{:?}", e),
        }
    }

    run(Version::V1);
    run(Version::V1Lazy);
}

#[test]
fn select_proto_parallel() {
    fn run(version: Version) {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map(|s| s.0.unwrap())
            .map_err(|(e, _)| e.into())
            .and_then(move |connec| {
                let protos = vec![b"/proto1", b"/proto2"];
                listener_select_proto(connec, protos)
            })
            .and_then(|(proto, io)| io.complete().map(move |_| proto));

        let client = TcpStream::connect(&listener_addr)
            .from_err()
            .and_then(move |connec| {
                let protos = vec![b"/proto3", b"/proto2"];
                dialer_select_proto_parallel(connec, protos.into_iter(), version)
            })
            .and_then(|(proto, io)| io.complete().map(move |_| proto));

        let mut rt = Runtime::new().unwrap();
        let (dialer_chosen, listener_chosen) =
            rt.block_on(client.join(server)).unwrap();

        assert_eq!(dialer_chosen, b"/proto2");
        assert_eq!(listener_chosen, b"/proto2");
    }

    run(Version::V1);
    run(Version::V1Lazy);
}

#[test]
fn select_proto_serial() {
    fn run(version: Version) {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map(|s| s.0.unwrap())
            .map_err(|(e, _)| e.into())
            .and_then(move |connec| {
                let protos = vec![b"/proto1", b"/proto2"];
                listener_select_proto(connec, protos)
            })
            .and_then(|(proto, io)| io.complete().map(move |_| proto));

        let client = TcpStream::connect(&listener_addr)
            .from_err()
            .and_then(move |connec| {
                let protos = vec![b"/proto3", b"/proto2"];
                dialer_select_proto_serial(connec, protos.into_iter(), version)
            })
            .and_then(|(proto, io)| io.complete().map(move |_| proto));

        let mut rt = Runtime::new().unwrap();
        let (dialer_chosen, listener_chosen) =
            rt.block_on(client.join(server)).unwrap();

        assert_eq!(dialer_chosen, b"/proto2");
        assert_eq!(listener_chosen, b"/proto2");
    }

    run(Version::V1);
    run(Version::V1Lazy);
}
