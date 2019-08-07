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

use futures::{future::Either, prelude::*, stream::StreamFuture};
use crate::protocol::{Dialer, DialerFuture, Request, Response};
use log::trace;
use std::mem;
use tokio_io::{AsyncRead, AsyncWrite};
use crate::{Negotiated, ProtocolChoiceError};

/// Future, returned by `dialer_select_proto`, which selects a protocol and dialer
/// either sequentially or by considering all protocols in parallel.
pub type DialerSelectFuture<R, I> = Either<DialerSelectSeq<R, I>, DialerSelectPar<R, I>>;

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
pub fn dialer_select_proto<R, I>(inner: R, protocols: I) -> DialerSelectFuture<R, I::IntoIter>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let iter = protocols.into_iter();
    // We choose between the "serial" and "parallel" strategies based on the number of protocols.
    if iter.size_hint().1.map(|n| n <= 3).unwrap_or(false) {
        Either::A(dialer_select_proto_serial(inner, iter))
    } else {
        Either::B(dialer_select_proto_parallel(inner, iter))
    }
}

/// Helps selecting a protocol amongst the ones supported.
///
/// Same as `dialer_select_proto`. Tries protocols one by one. The iterator doesn't need to produce
/// match functions, because it's not needed.
pub fn dialer_select_proto_serial<R, I>(inner: R, protocols: I) -> DialerSelectSeq<R, I::IntoIter>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let protocols = protocols.into_iter();
    DialerSelectSeq {
        inner: DialerSelectSeqState::AwaitDialer {
            dialer_fut: Dialer::dial(inner),
            protocols
        }
    }
}


/// Future, returned by `dialer_select_proto_serial` which selects a protocol
/// and dialer sequentially.
pub struct DialerSelectSeq<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    inner: DialerSelectSeqState<R, I>
}

enum DialerSelectSeqState<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    AwaitDialer {
        dialer_fut: DialerFuture<R, I::Item>,
        protocols: I
    },
    NextProtocol {
        dialer: Dialer<R, I::Item>,
        proto_name: I::Item,
        protocols: I
    },
    FlushProtocol {
        dialer: Dialer<R, I::Item>,
        proto_name: I::Item,
        protocols: I
    },
    AwaitProtocol {
        stream: StreamFuture<Dialer<R, I::Item>>,
        proto_name: I::Item,
        protocols: I
    },
    Undefined
}

impl<R, I> Future for DialerSelectSeq<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]> + Clone
{
    type Item = (I::Item, Negotiated<R>);
    type Error = ProtocolChoiceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, DialerSelectSeqState::Undefined) {
                DialerSelectSeqState::AwaitDialer { mut dialer_fut, mut protocols } => {
                    let dialer = match dialer_fut.poll()? {
                        Async::Ready(d) => d,
                        Async::NotReady => {
                            self.inner = DialerSelectSeqState::AwaitDialer { dialer_fut, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    let proto_name = protocols.next().ok_or(ProtocolChoiceError::NoProtocolFound)?;
                    self.inner = DialerSelectSeqState::NextProtocol {
                        dialer,
                        protocols,
                        proto_name
                    }
                }
                DialerSelectSeqState::NextProtocol { mut dialer, protocols, proto_name } => {
                    trace!("sending {:?}", proto_name.as_ref());
                    let req = Request::Protocol { name: proto_name.clone() };
                    match dialer.start_send(req)? {
                        AsyncSink::Ready => {
                            self.inner = DialerSelectSeqState::FlushProtocol {
                                dialer,
                                proto_name,
                                protocols
                            }
                        }
                        AsyncSink::NotReady(_) => {
                            self.inner = DialerSelectSeqState::NextProtocol {
                                dialer,
                                protocols,
                                proto_name
                            };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                DialerSelectSeqState::FlushProtocol { mut dialer, proto_name, protocols } => {
                    match dialer.poll_complete()? {
                        Async::Ready(()) => {
                            let stream = dialer.into_future();
                            self.inner = DialerSelectSeqState::AwaitProtocol {
                                stream,
                                proto_name,
                                protocols
                            }
                        }
                        Async::NotReady => {
                            self.inner = DialerSelectSeqState::FlushProtocol {
                                dialer,
                                proto_name,
                                protocols
                            };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                DialerSelectSeqState::AwaitProtocol { mut stream, proto_name, mut protocols } => {
                    let (m, r) = match stream.poll() {
                        Ok(Async::Ready(x)) => x,
                        Ok(Async::NotReady) => {
                            self.inner = DialerSelectSeqState::AwaitProtocol {
                                stream,
                                proto_name,
                                protocols
                            };
                            return Ok(Async::NotReady)
                        }
                        Err((e, _)) => return Err(ProtocolChoiceError::from(e))
                    };
                    trace!("received {:?}", m);
                    match m.ok_or(ProtocolChoiceError::UnexpectedMessage)? {
                        Response::Protocol { ref name }
                            if name.as_ref() == proto_name.as_ref() =>
                        {
                            return Ok(Async::Ready((proto_name, Negotiated(r.into_inner()))))
                        }
                        Response::ProtocolNotAvailable => {
                            let proto_name = protocols.next()
                                .ok_or(ProtocolChoiceError::NoProtocolFound)?;
                            self.inner = DialerSelectSeqState::NextProtocol {
                                dialer: r,
                                protocols,
                                proto_name
                            }
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
pub fn dialer_select_proto_parallel<R, I>(inner: R, protocols: I) -> DialerSelectPar<R, I::IntoIter>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let protocols = protocols.into_iter();
    DialerSelectPar {
        inner: DialerSelectParState::AwaitDialer { dialer_fut: Dialer::dial(inner), protocols }
    }
}

/// Future, returned by `dialer_select_proto_parallel`, which selects a protocol and dialer in
/// parallel, by first requesting the list of protocols supported by the remote endpoint and
/// then selecting the most appropriate one by applying a match predicate to the result.
pub struct DialerSelectPar<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    inner: DialerSelectParState<R, I>
}

enum DialerSelectParState<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    AwaitDialer {
        dialer_fut: DialerFuture<R, I::Item>,
        protocols: I
    },
    ProtocolList {
        dialer: Dialer<R, I::Item>,
        protocols: I
    },
    FlushListRequest {
        dialer: Dialer<R, I::Item>,
        protocols: I
    },
    AwaitListResponse {
        stream: StreamFuture<Dialer<R, I::Item>>,
        protocols: I,
    },
    Protocol {
        dialer: Dialer<R, I::Item>,
        proto_name: I::Item
    },
    FlushProtocol {
        dialer: Dialer<R, I::Item>,
        proto_name: I::Item
    },
    AwaitProtocol {
        stream: StreamFuture<Dialer<R, I::Item>>,
        proto_name: I::Item
    },
    Undefined
}

impl<R, I> Future for DialerSelectPar<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]> + Clone
{
    type Item = (I::Item, Negotiated<R>);
    type Error = ProtocolChoiceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, DialerSelectParState::Undefined) {
                DialerSelectParState::AwaitDialer { mut dialer_fut, protocols } => {
                    match dialer_fut.poll()? {
                        Async::Ready(dialer) => {
                            self.inner = DialerSelectParState::ProtocolList { dialer, protocols }
                        }
                        Async::NotReady => {
                            self.inner = DialerSelectParState::AwaitDialer { dialer_fut, protocols };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                DialerSelectParState::ProtocolList { mut dialer, protocols } => {
                    trace!("requesting protocols list");
                    match dialer.start_send(Request::ListProtocols)? {
                        AsyncSink::Ready => {
                            self.inner = DialerSelectParState::FlushListRequest {
                                dialer,
                                protocols
                            }
                        }
                        AsyncSink::NotReady(_) => {
                            self.inner = DialerSelectParState::ProtocolList { dialer, protocols };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                DialerSelectParState::FlushListRequest { mut dialer, protocols } => {
                    match dialer.poll_complete()? {
                        Async::Ready(()) => {
                            self.inner = DialerSelectParState::AwaitListResponse {
                                stream: dialer.into_future(),
                                protocols
                            }
                        }
                        Async::NotReady => {
                            self.inner = DialerSelectParState::FlushListRequest {
                                dialer,
                                protocols
                            };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                DialerSelectParState::AwaitListResponse { mut stream, protocols } => {
                    let (resp, dialer) = match stream.poll() {
                        Ok(Async::Ready(x)) => x,
                        Ok(Async::NotReady) => {
                            self.inner = DialerSelectParState::AwaitListResponse { stream, protocols };
                            return Ok(Async::NotReady)
                        }
                        Err((e, _)) => return Err(ProtocolChoiceError::from(e))
                    };
                    trace!("protocols list response: {:?}", resp);
                    let supported =
                        if let Some(Response::SupportedProtocols { protocols }) = resp {
                            protocols
                        } else {
                            return Err(ProtocolChoiceError::UnexpectedMessage)
                        };
                    let mut found = None;
                    for local_name in protocols {
                        for remote_name in &supported {
                            if remote_name.as_ref() == local_name.as_ref() {
                                found = Some(local_name);
                                break;
                            }
                        }
                        if found.is_some() {
                            break;
                        }
                    }
                    let proto_name = found.ok_or(ProtocolChoiceError::NoProtocolFound)?;
                    self.inner = DialerSelectParState::Protocol { dialer, proto_name }
                }
                DialerSelectParState::Protocol { mut dialer, proto_name } => {
                    trace!("Requesting protocol: {:?}", proto_name.as_ref());
                    let req = Request::Protocol { name: proto_name.clone() };
                    match dialer.start_send(req)? {
                        AsyncSink::Ready => {
                            self.inner = DialerSelectParState::FlushProtocol { dialer, proto_name }
                        }
                        AsyncSink::NotReady(_) => {
                            self.inner = DialerSelectParState::Protocol { dialer, proto_name };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                DialerSelectParState::FlushProtocol { mut dialer, proto_name } => {
                    match dialer.poll_complete()? {
                        Async::Ready(()) => {
                            self.inner = DialerSelectParState::AwaitProtocol {
                                stream: dialer.into_future(),
                                proto_name
                            }
                        }
                        Async::NotReady => {
                            self.inner = DialerSelectParState::FlushProtocol { dialer, proto_name };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                DialerSelectParState::AwaitProtocol { mut stream, proto_name } => {
                    let (resp, dialer) = match stream.poll() {
                        Ok(Async::Ready(x)) => x,
                        Ok(Async::NotReady) => {
                            self.inner = DialerSelectParState::AwaitProtocol { stream, proto_name };
                            return Ok(Async::NotReady)
                        }
                        Err((e, _)) => return Err(ProtocolChoiceError::from(e))
                    };
                    trace!("received {:?}", resp);
                    match resp {
                        Some(Response::Protocol { ref name })
                            if name.as_ref() == proto_name.as_ref() =>
                        {
                            return Ok(Async::Ready((proto_name, Negotiated(dialer.into_inner()))))
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

