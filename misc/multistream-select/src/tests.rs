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

use tokio::runtime::current_thread::Runtime;
use tokio_tcp::{TcpListener, TcpStream};
use bytes::Bytes;
use crate::dialer_select::{dialer_select_proto_parallel, dialer_select_proto_serial};
use futures::Future;
use futures::{Sink, Stream};
use crate::protocol::{Dialer, DialerToListenerMessage, Listener, ListenerToDialerMessage};
use crate::ProtocolChoiceError;
use crate::{dialer_select_proto, listener_select_proto};

/// Holds a `Vec` and satifies the iterator requirements of `listener_select_proto`.
struct VecRefIntoIter<T>(Vec<T>);

impl<'a, T> IntoIterator for &'a VecRefIntoIter<T>
where T: Clone
{
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.clone().into_iter()
    }
}

#[test]
fn negotiate_with_self_succeeds() {
    let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let listener_addr = listener.local_addr().unwrap();

    let server = listener
        .incoming()
        .into_future()
        .map_err(|(e, _)| e.into())
        .and_then(move |(connec, _)| Listener::new(connec.unwrap()))
        .and_then(|l| l.into_future().map_err(|(e, _)| e))
        .and_then(|(msg, rest)| {
            let proto = match msg {
                Some(DialerToListenerMessage::ProtocolRequest { name }) => name,
                _ => panic!(),
            };
            rest.send(ListenerToDialerMessage::ProtocolAck { name: proto })
        });

    let client = TcpStream::connect(&listener_addr)
        .from_err()
        .and_then(move |stream| Dialer::new(stream))
        .and_then(move |dialer| {
            let p = Bytes::from("/hello/1.0.0");
            dialer.send(DialerToListenerMessage::ProtocolRequest { name: p })
        })
        .and_then(move |dialer| dialer.into_future().map_err(|(e, _)| e))
        .and_then(move |(msg, _)| {
            let proto = match msg {
                Some(ListenerToDialerMessage::ProtocolAck { name }) => name,
                _ => panic!(),
            };
            assert_eq!(proto, "/hello/1.0.0");
            Ok(())
        });
    let mut rt = Runtime::new().unwrap();
    let _ = rt.block_on(server.join(client)).unwrap();
}

#[test]
fn select_proto_basic() {
    let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let listener_addr = listener.local_addr().unwrap();

    let server = listener
        .incoming()
        .into_future()
        .map(|s| s.0.unwrap())
        .map_err(|(e, _)| e.into())
        .and_then(move |connec| {
            let protos = vec![
                (Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 0),
                (Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 1),
            ];
            listener_select_proto(connec, VecRefIntoIter(protos)).map(|r| r.0)
        });

    let client = TcpStream::connect(&listener_addr)
        .from_err()
        .and_then(move |connec| {
            let protos = vec![
                (Bytes::from("/proto3"), <Bytes as PartialEq>::eq, 2),
                (Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 3),
            ].into_iter();
            dialer_select_proto(connec, protos).map(|r| r.0)
        });
    let mut rt = Runtime::new().unwrap();
    let (dialer_chosen, listener_chosen) =
        rt.block_on(client.join(server)).unwrap();
    assert_eq!(dialer_chosen, 3);
    assert_eq!(listener_chosen, 1);
}

#[test]
fn no_protocol_found() {
    let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let listener_addr = listener.local_addr().unwrap();

    let server = listener
        .incoming()
        .into_future()
        .map(|s| s.0.unwrap())
        .map_err(|(e, _)| e.into())
        .and_then(move |connec| {
            let protos = vec![
                (Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 1),
                (Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 2),
            ];
            listener_select_proto(connec, VecRefIntoIter(protos)).map(|r| r.0)
        });

    let client = TcpStream::connect(&listener_addr)
        .from_err()
        .and_then(move |connec| {
            let protos = vec![
                (Bytes::from("/proto3"), <Bytes as PartialEq>::eq, 3),
                (Bytes::from("/proto4"), <Bytes as PartialEq>::eq, 4),
            ].into_iter();
            dialer_select_proto(connec, protos).map(|r| r.0)
        });
    let mut rt = Runtime::new().unwrap();
    match rt.block_on(client.join(server)) {
        Err(ProtocolChoiceError::NoProtocolFound) => (),
        _ => panic!(),
    }
}

#[test]
fn select_proto_parallel() {
    let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let listener_addr = listener.local_addr().unwrap();

    let server = listener
        .incoming()
        .into_future()
        .map(|s| s.0.unwrap())
        .map_err(|(e, _)| e.into())
        .and_then(move |connec| {
            let protos = vec![
                (Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 0),
                (Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 1),
            ];
            listener_select_proto(connec, VecRefIntoIter(protos)).map(|r| r.0)
        });

    let client = TcpStream::connect(&listener_addr)
        .from_err()
        .and_then(move |connec| {
            let protos = vec![
                (Bytes::from("/proto3"), <Bytes as PartialEq>::eq, 2),
                (Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 3),
            ].into_iter();
            dialer_select_proto_parallel(connec, protos).map(|r| r.0)
        });

    let mut rt = Runtime::new().unwrap();
    let (dialer_chosen, listener_chosen) =
        rt.block_on(client.join(server)).unwrap();
    assert_eq!(dialer_chosen, 3);
    assert_eq!(listener_chosen, 1);
}

#[test]
fn select_proto_serial() {
    let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
    let listener_addr = listener.local_addr().unwrap();

    let server = listener
        .incoming()
        .into_future()
        .map(|s| s.0.unwrap())
        .map_err(|(e, _)| e.into())
        .and_then(move |connec| {
            let protos = vec![
                (Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 0),
                (Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 1),
            ];
            listener_select_proto(connec, VecRefIntoIter(protos)).map(|r| r.0)
        });

    let client = TcpStream::connect(&listener_addr)
        .from_err()
        .and_then(move |connec| {
            let protos = vec![(Bytes::from("/proto3"), 2), (Bytes::from("/proto2"), 3)].into_iter();
            dialer_select_proto_serial(connec, protos).map(|r| r.0)
        });

    let mut rt = Runtime::new().unwrap();
    let (dialer_chosen, listener_chosen) =
        rt.block_on(client.join(server)).unwrap();
    assert_eq!(dialer_chosen, 3);
    assert_eq!(listener_chosen, 1);
}
