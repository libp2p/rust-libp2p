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

//! Contains the `listener_select_proto` code, which allows selecting a protocol thanks to
//! `multistream-select` for the listener.

use bytes::Bytes;
use futures::{prelude::*, sink, stream::StreamFuture};
use crate::protocol::{DialerToListenerMessage, Listener, ListenerFuture, ListenerToDialerMessage};
use log::{debug, trace};
use std::mem;
use tokio_io::{AsyncRead, AsyncWrite};
use crate::ProtocolChoiceError;

/// Helps selecting a protocol amongst the ones supported.
///
/// This function expects a socket and an iterator of the list of supported protocols. The iterator
/// must be clonable (i.e. iterable multiple times), because the list may need to be accessed
/// multiple times.
///
/// The iterator must produce tuples of the name of the protocol that is advertised to the remote,
/// a function that will check whether a remote protocol matches ours, and an identifier for the
/// protocol of type `P` (you decide what `P` is). The parameters of the function are the name
/// proposed by the remote, and the protocol name that we passed (so that you don't have to clone
/// the name).
///
/// On success, returns the socket and the identifier of the chosen protocol (of type `P`). The
/// socket now uses this protocol.
pub fn listener_select_proto<R, I, M, P>(inner: R, protocols: I) -> ListenerSelectFuture<R, I, P>
where
    R: AsyncRead + AsyncWrite,
    for<'r> &'r I: IntoIterator<Item = (Bytes, M, P)>,
    M: FnMut(&Bytes, &Bytes) -> bool,
{
    ListenerSelectFuture {
        inner: ListenerSelectState::AwaitListener { listener_fut: Listener::new(inner), protocols }
    }
}

/// Future, returned by `listener_select_proto` which selects a protocol among the ones supported.
pub struct ListenerSelectFuture<R: AsyncRead + AsyncWrite, I, P> {
    inner: ListenerSelectState<R, I, P>
}

enum ListenerSelectState<R: AsyncRead + AsyncWrite, I, P> {
    AwaitListener {
        listener_fut: ListenerFuture<R>,
        protocols: I
    },
    Incoming {
        stream: StreamFuture<Listener<R>>,
        protocols: I
    },
    Outgoing {
        sender: sink::Send<Listener<R>>,
        protocols: I,
        outcome: Option<P>
    },
    Undefined
}

impl<R, I, M, P> Future for ListenerSelectFuture<R, I, P>
where
    for<'r> &'r I: IntoIterator<Item=(Bytes, M, P)>,
    M: FnMut(&Bytes, &Bytes) -> bool,
    R: AsyncRead + AsyncWrite,
{
    type Item = (P, R, I);
    type Error = ProtocolChoiceError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, ListenerSelectState::Undefined) {
                ListenerSelectState::AwaitListener { mut listener_fut, protocols } => {
                    let listener = match listener_fut.poll()? {
                        Async::Ready(l) => l,
                        Async::NotReady => {
                            self.inner = ListenerSelectState::AwaitListener { listener_fut, protocols };
                            return Ok(Async::NotReady)
                        }
                    };
                    let stream = listener.into_future();
                    self.inner = ListenerSelectState::Incoming { stream, protocols };
                }
                ListenerSelectState::Incoming { mut stream, protocols } => {
                    let (msg, listener) = match stream.poll() {
                        Ok(Async::Ready(x)) => x,
                        Ok(Async::NotReady) => {
                            self.inner = ListenerSelectState::Incoming { stream, protocols };
                            return Ok(Async::NotReady)
                        }
                        Err((e, _)) => return Err(ProtocolChoiceError::from(e))
                    };
                    match msg {
                        Some(DialerToListenerMessage::ProtocolsListRequest) => {
                            let msg = ListenerToDialerMessage::ProtocolsListResponse {
                                list: protocols.into_iter().map(|(p, _, _)| p).collect(),
                            };
                            trace!("protocols list response: {:?}", msg);
                            let sender = listener.send(msg);
                            self.inner = ListenerSelectState::Outgoing {
                                sender,
                                protocols,
                                outcome: None
                            }
                        }
                        Some(DialerToListenerMessage::ProtocolRequest { name }) => {
                            let mut outcome = None;
                            let mut send_back = ListenerToDialerMessage::NotAvailable;
                            for (supported, mut matches, value) in &protocols {
                                if matches(&name, &supported) {
                                    send_back = ListenerToDialerMessage::ProtocolAck {name: name.clone()};
                                    outcome = Some(value);
                                    break;
                                }
                            }
                            trace!("requested: {:?}, response: {:?}", name, send_back);
                            let sender = listener.send(send_back);
                            self.inner = ListenerSelectState::Outgoing { sender, protocols, outcome }
                        }
                        None => {
                            debug!("no protocol request received");
                            return Err(ProtocolChoiceError::NoProtocolFound)
                        }
                    }
                }
                ListenerSelectState::Outgoing { mut sender, protocols, outcome } => {
                    let listener = match sender.poll()? {
                        Async::Ready(l) => l,
                        Async::NotReady => {
                            self.inner = ListenerSelectState::Outgoing { sender, protocols, outcome };
                            return Ok(Async::NotReady)
                        }
                    };
                    if let Some(p) = outcome {
                        return Ok(Async::Ready((p, listener.into_inner(), protocols)))
                    } else {
                        let stream = listener.into_future();
                        self.inner = ListenerSelectState::Incoming { stream, protocols }
                    }
                }
                ListenerSelectState::Undefined =>
                    panic!("ListenerSelectState::poll called after completion")
            }
        }
    }
}
