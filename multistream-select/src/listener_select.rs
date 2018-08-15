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
use futures::future::{err, loop_fn, Loop, Either};
use futures::{Future, Sink, Stream};
use ProtocolChoiceError;

use protocol::DialerToListenerMessage;
use protocol::Listener;
use protocol::ListenerToDialerMessage;
use tokio_io::{AsyncRead, AsyncWrite};

/// Helps selecting a protocol amongst the ones supported.
///
/// This function expects a socket and an iterator of the list of supported protocols. The iterator
/// must be clonable (ie. iterable multiple times), because the list may need to be accessed
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
pub fn listener_select_proto<R, I, M, P>(
    inner: R,
    protocols: I,
) -> impl Future<Item = (P, R), Error = ProtocolChoiceError>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item = (Bytes, M, P)> + Clone,
    M: FnMut(&Bytes, &Bytes) -> bool,
{
    Listener::new(inner).from_err().and_then(move |listener| {
        loop_fn(listener, move |listener| {
            let protocols = protocols.clone();

            listener
                .into_future()
                .map_err(|(e, _)| e.into())
                .and_then(move |(message, listener)| match message {
                    Some(DialerToListenerMessage::ProtocolsListRequest) => {
                        let msg = ListenerToDialerMessage::ProtocolsListResponse {
                            list: protocols.map(|(p, _, _)| p).collect(),
                        };
                        trace!("protocols list response: {:?}", msg);
                        let fut = listener
                            .send(msg)
                            .from_err()
                            .map(move |listener| (None, listener));
                        Either::A(Either::A(fut))
                    }
                    Some(DialerToListenerMessage::ProtocolRequest { name }) => {
                        let mut outcome = None;
                        let mut send_back = ListenerToDialerMessage::NotAvailable;
                        for (supported, mut matches, value) in protocols {
                            if matches(&name, &supported) {
                                send_back =
                                    ListenerToDialerMessage::ProtocolAck { name: name.clone() };
                                outcome = Some(value);
                                break;
                            }
                        }
                        trace!("requested: {:?}, response: {:?}", name, send_back);
                        let fut = listener
                            .send(send_back)
                            .from_err()
                            .map(move |listener| (outcome, listener));
                        Either::A(Either::B(fut))
                    }
                    None => {
                        debug!("no protocol request received");
                        Either::B(err(ProtocolChoiceError::NoProtocolFound))
                    }
                })
                .map(|(outcome, listener): (_, Listener<R>)| match outcome {
                    Some(outcome) => Loop::Break((outcome, listener.into_inner())),
                    None => Loop::Continue(listener),
                })
        })
    })
}
