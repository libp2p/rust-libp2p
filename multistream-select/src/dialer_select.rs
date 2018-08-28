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


pub struct IgnoreMatchFn<I>(I);

impl<I, M, P> Iterator for IgnoreMatchFn<I>
where
    I: Iterator<Item=(Bytes, M, P)>
{
    type Item = (Bytes, P);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(b, _, p)| (b, p))
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
    DialerSelectSeq::AwaitDialer { dialer_fut: Dialer::new(inner), protocols }
}


pub enum DialerSelectSeq<R: AsyncRead + AsyncWrite, I, P> {
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
            match mem::replace(self, DialerSelectSeq::Undefined) {
                DialerSelectSeq::AwaitDialer { mut dialer_fut, protocols } => {
                    let dialer = match dialer_fut.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            *self = DialerSelectSeq::AwaitDialer { dialer_fut, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    *self = DialerSelectSeq::NextProtocol { dialer, protocols }
                }
                DialerSelectSeq::NextProtocol { dialer, mut protocols } => {
                    let (proto_name, proto_value) =
                        protocols.next().ok_or(ProtocolChoiceError::NoProtocolFound)?;
                    let req = DialerToListenerMessage::ProtocolRequest {
                        name: proto_name.clone()
                    };
                    trace!("sending {:?}", req);
                    let sender = dialer.send(req);
                    *self = DialerSelectSeq::SendProtocol { sender, proto_name, proto_value, protocols }
                }
                DialerSelectSeq::SendProtocol { mut sender, proto_name, proto_value, protocols } => {
                    let dialer = match sender.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            *self = DialerSelectSeq::SendProtocol {
                                sender,
                                proto_name,
                                proto_value,
                                protocols
                            };
                            return Ok(Async::NotReady)
                        }
                    };
                    let stream = dialer.into_future();
                    *self = DialerSelectSeq::AwaitProtocol {
                        stream,
                        proto_name,
                        proto_value,
                        protocols
                    };
                }
                DialerSelectSeq::AwaitProtocol { mut stream, proto_name, proto_value, protocols } => {
                    let (m, r) = match stream.poll().map_err(|(e, _)| ProtocolChoiceError::from(e))? {
                        Async::Ready(x) => x,
                        Async::NotReady => {
                            *self = DialerSelectSeq::AwaitProtocol {
                                stream,
                                proto_name,
                                proto_value,
                                protocols
                            };
                            return Ok(Async::NotReady)
                        }
                    };
                    trace!("received {:?}", m);
                    match m.ok_or(ProtocolChoiceError::UnexpectedMessage)? {
                        ListenerToDialerMessage::ProtocolAck { ref name } if name == &proto_name => {
                            return Ok(Async::Ready((proto_value, r.into_inner())))
                        },
                        ListenerToDialerMessage::NotAvailable => {
                            *self = DialerSelectSeq::NextProtocol { dialer: r, protocols }
                        }
                        _ => return Err(ProtocolChoiceError::UnexpectedMessage)
                    }
                }
                DialerSelectSeq::Undefined => panic!("DialerSelectSeq::poll called after completion")
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
    DialerSelectPar::AwaitDialer { dialer_fut: Dialer::new(inner), protocols }
}


pub enum DialerSelectPar<R: AsyncRead + AsyncWrite, I, P> {
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
            match mem::replace(self, DialerSelectPar::Undefined) {
                DialerSelectPar::AwaitDialer { mut dialer_fut, protocols } => {
                    let dialer = match dialer_fut.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            *self = DialerSelectPar::AwaitDialer { dialer_fut, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    trace!("requesting protocols list");
                    let sender = dialer.send(DialerToListenerMessage::ProtocolsListRequest);
                    *self = DialerSelectPar::SendRequest { sender, protocols };
                }
                DialerSelectPar::SendRequest { mut sender, protocols } => {
                    let dialer = match sender.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            *self = DialerSelectPar::SendRequest { sender, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    let stream = dialer.into_future();
                    *self = DialerSelectPar::AwaitResponse { stream, protocols };
                }
                DialerSelectPar::AwaitResponse { mut stream, protocols } => {
                    let (m, d) = match stream.poll().map_err(|(e, _)| ProtocolChoiceError::from(e))? {
                        Async::Ready(x) => x,
                        Async::NotReady => {
                            *self = DialerSelectPar::AwaitResponse { stream, protocols };
                            return Ok(Async::NotReady)
                        }
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
                    *self = DialerSelectPar::SendProtocol { sender, proto_name, proto_val };
                }
                DialerSelectPar::SendProtocol { mut sender, proto_name, proto_val } => {
                    let dialer = match sender.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            *self = DialerSelectPar::SendProtocol { sender, proto_name, proto_val };
                            return Ok(Async::NotReady)
                        }
                    };
                    let stream = dialer.into_future();
                    *self = DialerSelectPar::AwaitProtocol { stream, proto_name, proto_val };
                }
                DialerSelectPar::AwaitProtocol { mut stream, proto_name, proto_val } => {
                    let (m, r) = match stream.poll().map_err(|(e, _)| ProtocolChoiceError::from(e))? {
                        Async::Ready(x) => x,
                        Async::NotReady => {
                            *self = DialerSelectPar::AwaitProtocol { stream, proto_name, proto_val };
                            return Ok(Async::NotReady)
                        }
                    };
                    trace!("received {:?}", m);
                    match m {
                        Some(ListenerToDialerMessage::ProtocolAck { ref name }) if name == &proto_name => {
                            return Ok(Async::Ready((proto_val, r.into_inner())))
                        }
                        _ => return Err(ProtocolChoiceError::UnexpectedMessage)
                    }
                }
                DialerSelectPar::Undefined => panic!("DialerSelectPar::poll called after completion")
            }
        }
    }
}


