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

//! Contains the `dialer_select_proto` code, which allows selecting a protocol thanks to
//! `multistream-select` for the dialer.

use bytes::Bytes;
use futures::{future::Either, prelude::*, sink, stream::StreamFuture};
use protocol::{Dialer, DialerFuture, DialerToListenerMessage, ListenerToDialerMessage};
use std::mem;
use tokio_io::{AsyncRead, AsyncWrite};
use ProtocolChoiceError;

/// Future, returned by `dialer_select_proto`, which selects a protocol and dialer
/// either sequentially of by considering all protocols in parallel.
pub type DialerSelectFuture<R, I, P> =
    Either<DialerSelectSeq<R, IgnoreMatchFn<I>, P>, DialerSelectPar<R, I, P>>;

/// Helps selecting a protocol amongst the ones supported.
///
/// This function expects a socket and a list of protocols. It uses the `multistream-select`
/// protocol to choose with the remote a protocol amongst the ones produced by the iterator.
///
/// The iterator must produce a tuple of a protocol name advertised to the remote, a function that
/// checks whether a protocol name matches the protocol, and a protocol "identifier" of type `P`
/// (you decide what `P` is). The parameters of the match function are the name proposed by the
/// remote, and the protocol name that we passed (so that you don't have to clone the name). On
/// success, the function returns the identifier (of type `P`), plus the socket which now uses that
/// chosen protocol.
#[inline]
pub fn dialer_select_proto<R, I, M, P>(inner: R, protocols: I) -> DialerSelectFuture<R, I, P>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item=(Bytes, M, P)>,
    M: FnMut(&Bytes, &Bytes) -> bool,
{
    // We choose between the "serial" and "parallel" strategies based on the number of protocols.
    if protocols.size_hint().1.map(|n| n <= 3).unwrap_or(false) {
        Either::A(dialer_select_proto_serial(inner, IgnoreMatchFn(protocols)))
    } else {
        Either::B(dialer_select_proto_parallel(inner, protocols))
    }
}


/// Iterator, which ignores match predicates of the iterator it wraps.
pub struct IgnoreMatchFn<I>(I);

impl<I, M, P> Iterator for IgnoreMatchFn<I>
where
    I: Iterator<Item=(Bytes, M, P)>
{
    type Item = (Bytes, P);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(b, _, p)| (b, p))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}


/// Helps selecting a protocol amongst the ones supported.
///
/// Same as `dialer_select_proto`. Tries protocols one by one. The iterator doesn't need to produce
/// match functions, because it's not needed.
pub fn dialer_select_proto_serial<R, I, P>(inner: R, protocols: I,) -> DialerSelectSeq<R, I, P>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item = (Bytes, P)>,
{
    DialerSelectSeq {
        inner: DialerSelectSeqState::AwaitDialer { dialer_fut: Dialer::new(inner), protocols }
    }
}


/// Future, returned by `dialer_select_proto_serial` which selects a protocol
/// and dialer sequentially.
pub struct DialerSelectSeq<R: AsyncRead + AsyncWrite, I, P> {
    inner: DialerSelectSeqState<R, I, P>
}

enum DialerSelectSeqState<R: AsyncRead + AsyncWrite, I, P> {
    AwaitDialer {
        dialer_fut: DialerFuture<R>,
        protocols: I
    },
    NextProtocol {
        dialer: Dialer<R>,
        protocols: I
    },
    SendProtocol {
        sender: sink::Send<Dialer<R>>,
        proto_name: Bytes,
        proto_value: P,
        protocols: I
    },
    AwaitProtocol {
        stream: StreamFuture<Dialer<R>>,
        proto_name: Bytes,
        proto_value: P,
        protocols: I
    },
    Undefined
}

impl<R, I, P> Future for DialerSelectSeq<R, I, P>
where
    I: Iterator<Item=(Bytes, P)>,
    R: AsyncRead + AsyncWrite,
{
    type Item = (P, R);
    type Error = ProtocolChoiceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, DialerSelectSeqState::Undefined) {
                DialerSelectSeqState::AwaitDialer { mut dialer_fut, protocols } => {
                    let dialer = match dialer_fut.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            self.inner = DialerSelectSeqState::AwaitDialer { dialer_fut, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    self.inner = DialerSelectSeqState::NextProtocol { dialer, protocols }
                }
                DialerSelectSeqState::NextProtocol { dialer, mut protocols } => {
                    let (proto_name, proto_value) =
                        protocols.next().ok_or(ProtocolChoiceError::NoProtocolFound)?;
                    let req = DialerToListenerMessage::ProtocolRequest {
                        name: proto_name.clone()
                    };
                    trace!("sending {:?}", req);
                    let sender = dialer.send(req);
                    self.inner = DialerSelectSeqState::SendProtocol {
                        sender,
                        proto_name,
                        proto_value,
                        protocols
                    }
                }
                DialerSelectSeqState::SendProtocol { mut sender, proto_name, proto_value, protocols } => {
                    let dialer = match sender.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            self.inner = DialerSelectSeqState::SendProtocol {
                                sender,
                                proto_name,
                                proto_value,
                                protocols
                            };
                            return Ok(Async::NotReady)
                        }
                    };
                    let stream = dialer.into_future();
                    self.inner = DialerSelectSeqState::AwaitProtocol {
                        stream,
                        proto_name,
                        proto_value,
                        protocols
                    };
                }
                DialerSelectSeqState::AwaitProtocol { mut stream, proto_name, proto_value, protocols } => {
                    let (m, r) = match stream.poll() {
                        Ok(Async::Ready(x)) => x,
                        Ok(Async::NotReady) => {
                            self.inner = DialerSelectSeqState::AwaitProtocol {
                                stream,
                                proto_name,
                                proto_value,
                                protocols
                            };
                            return Ok(Async::NotReady)
                        }
                        Err((e, _)) => return Err(ProtocolChoiceError::from(e))
                    };
                    trace!("received {:?}", m);
                    match m.ok_or(ProtocolChoiceError::UnexpectedMessage)? {
                        ListenerToDialerMessage::ProtocolAck { ref name } if name == &proto_name => {
                            return Ok(Async::Ready((proto_value, r.into_inner())))
                        },
                        ListenerToDialerMessage::NotAvailable => {
                            self.inner = DialerSelectSeqState::NextProtocol { dialer: r, protocols }
                        }
                        _ => return Err(ProtocolChoiceError::UnexpectedMessage)
                    }
                }
                DialerSelectSeqState::Undefined =>
                    panic!("DialerSelectSeqState::poll called after completion")
            }
        }
    }
}


/// Helps selecting a protocol amongst the ones supported.
///
/// Same as `dialer_select_proto`. Queries the list of supported protocols from the remote, then
/// chooses the most appropriate one.
pub fn dialer_select_proto_parallel<R, I, M, P>(inner: R, protocols: I) -> DialerSelectPar<R, I, P>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item = (Bytes, M, P)>,
    M: FnMut(&Bytes, &Bytes) -> bool,
{
    DialerSelectPar {
        inner: DialerSelectParState::AwaitDialer { dialer_fut: Dialer::new(inner), protocols }
    }
}


/// Future, returned by `dialer_select_proto_parallel`, which selects a protocol and dialer in
/// parellel, by first requesting the liste of protocols supported by the remote endpoint and
/// then selecting the most appropriate one by applying a match predicate to the result.
pub struct DialerSelectPar<R: AsyncRead + AsyncWrite, I, P> {
    inner: DialerSelectParState<R, I, P>
}

enum DialerSelectParState<R: AsyncRead + AsyncWrite, I, P> {
    AwaitDialer {
        dialer_fut: DialerFuture<R>,
        protocols: I
    },
    SendRequest {
        sender: sink::Send<Dialer<R>>,
        protocols: I
    },
    AwaitResponse {
        stream: StreamFuture<Dialer<R>>,
        protocols: I
    },
    SendProtocol {
        sender: sink::Send<Dialer<R>>,
        proto_name: Bytes,
        proto_val: P
    },
    AwaitProtocol {
        stream: StreamFuture<Dialer<R>>,
        proto_name: Bytes,
        proto_val: P
    },
    Undefined
}

impl<R, I, M, P> Future for DialerSelectPar<R, I, P>
where
    I: Iterator<Item=(Bytes, M, P)>,
    M: FnMut(&Bytes, &Bytes) -> bool,
    R: AsyncRead + AsyncWrite,
{
    type Item = (P, R);
    type Error = ProtocolChoiceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, DialerSelectParState::Undefined) {
                DialerSelectParState::AwaitDialer { mut dialer_fut, protocols } => {
                    let dialer = match dialer_fut.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            self.inner = DialerSelectParState::AwaitDialer { dialer_fut, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    trace!("requesting protocols list");
                    let sender = dialer.send(DialerToListenerMessage::ProtocolsListRequest);
                    self.inner = DialerSelectParState::SendRequest { sender, protocols };
                }
                DialerSelectParState::SendRequest { mut sender, protocols } => {
                    let dialer = match sender.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            self.inner = DialerSelectParState::SendRequest { sender, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    let stream = dialer.into_future();
                    self.inner = DialerSelectParState::AwaitResponse { stream, protocols };
                }
                DialerSelectParState::AwaitResponse { mut stream, protocols } => {
                    let (m, d) = match stream.poll() {
                        Ok(Async::Ready(x)) => x,
                        Ok(Async::NotReady) => {
                            self.inner = DialerSelectParState::AwaitResponse { stream, protocols };
                            return Ok(Async::NotReady)
                        }
                        Err((e, _)) => return Err(ProtocolChoiceError::from(e))
                    };
                    trace!("protocols list response: {:?}", m);
                    let list = match m {
                        Some(ListenerToDialerMessage::ProtocolsListResponse { list }) => list,
                        _ => return Err(ProtocolChoiceError::UnexpectedMessage),
                    };
                    let mut found = None;
                    for (local_name, mut match_fn, ident) in protocols {
                        for remote_name in &list {
                            if match_fn(remote_name, &local_name) {
                                found = Some((remote_name.clone(), ident));
                                break;
                            }
                        }
                        if found.is_some() {
                            break;
                        }
                    }
                    let (proto_name, proto_val) = found.ok_or(ProtocolChoiceError::NoProtocolFound)?;
                    trace!("sending {:?}", proto_name);
                    let sender = d.send(DialerToListenerMessage::ProtocolRequest {
                        name: proto_name.clone(),
                    });
                    self.inner = DialerSelectParState::SendProtocol { sender, proto_name, proto_val };
                }
                DialerSelectParState::SendProtocol { mut sender, proto_name, proto_val } => {
                    let dialer = match sender.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            self.inner = DialerSelectParState::SendProtocol {
                                sender,
                                proto_name,
                                proto_val
                            };
                            return Ok(Async::NotReady)
                        }
                    };
                    let stream = dialer.into_future();
                    self.inner = DialerSelectParState::AwaitProtocol {
                        stream,
                        proto_name,
                        proto_val
                    };
                }
                DialerSelectParState::AwaitProtocol { mut stream, proto_name, proto_val } => {
                    let (m, r) = match stream.poll() {
                        Ok(Async::Ready(x)) => x,
                        Ok(Async::NotReady) => {
                            self.inner = DialerSelectParState::AwaitProtocol {
                                stream,
                                proto_name,
                                proto_val
                            };
                            return Ok(Async::NotReady)
                        }
                        Err((e, _)) => return Err(ProtocolChoiceError::from(e))
                    };
                    trace!("received {:?}", m);
                    match m {
                        Some(ListenerToDialerMessage::ProtocolAck { ref name }) if name == &proto_name => {
                            return Ok(Async::Ready((proto_val, r.into_inner())))
                        }
                        _ => return Err(ProtocolChoiceError::UnexpectedMessage)
                    }
                }
                DialerSelectParState::Undefined =>
                    panic!("DialerSelectParState::poll called after completion")
            }
        }
    }
}


