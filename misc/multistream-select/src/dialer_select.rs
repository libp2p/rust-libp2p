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
use futures::future::{loop_fn, result, Loop, Either};
use futures::{Future, Sink, Stream};
use ProtocolChoiceError;

use protocol::Dialer;
use protocol::DialerToListenerMessage;
use protocol::ListenerToDialerMessage;
use tokio_io::{AsyncRead, AsyncWrite};

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
pub fn dialer_select_proto<R, I, M, P>(
    inner: R,
    protocols: I,
) -> impl Future<Item = (P, R), Error = ProtocolChoiceError>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item = (Bytes, M, P)>,
    M: FnMut(&Bytes, &Bytes) -> bool,
{
    // We choose between the "serial" and "parallel" strategies based on the number of protocols.
    if protocols.size_hint().1.map(|n| n <= 3).unwrap_or(false) {
        let fut = dialer_select_proto_serial(inner, protocols.map(|(n, _, id)| (n, id)));
        Either::A(fut)
    } else {
        let fut = dialer_select_proto_parallel(inner, protocols);
        Either::B(fut)
    }
}

/// Helps selecting a protocol amongst the ones supported.
///
/// Same as `dialer_select_proto`. Tries protocols one by one. The iterator doesn't need to produce
/// match functions, because it's not needed.
pub fn dialer_select_proto_serial<R, I, P>(
    inner: R,
    mut protocols: I,
) -> impl Future<Item = (P, R), Error = ProtocolChoiceError>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item = (Bytes, P)>,
{
    Dialer::new(inner).from_err().and_then(move |dialer| {
        // Similar to a `loop` keyword.
        loop_fn(dialer, move |dialer| {
            result(protocols.next().ok_or(ProtocolChoiceError::NoProtocolFound))
                    // If the `protocols` iterator produced an element, send it to the dialer
                    .and_then(|(proto_name, proto_value)| {
                        let req = DialerToListenerMessage::ProtocolRequest {
                            name: proto_name.clone()
                        };
                        trace!("sending {:?}", req);
                        dialer.send(req)
                            .map(|d| (d, proto_name, proto_value))
                            .from_err()
                    })
                    // Once sent, read one element from `dialer`.
                    .and_then(|(dialer, proto_name, proto_value)| {
                        dialer
                            .into_future()
                            .map(|(msg, rest)| (msg, rest, proto_name, proto_value))
                            .map_err(|(e, _)| e.into())
                    })
                    // Once read, analyze the response.
                    .and_then(|(message, rest, proto_name, proto_value)| {
                        trace!("received {:?}", message);
                        let message = message.ok_or(ProtocolChoiceError::UnexpectedMessage)?;

                        match message {
                            ListenerToDialerMessage::ProtocolAck { ref name }
                                                                    if name == &proto_name =>
                            {
                                // Satisfactory response, break the loop.
                                Ok(Loop::Break((proto_value, rest.into_inner())))
                            },
                            ListenerToDialerMessage::NotAvailable => {
                                Ok(Loop::Continue(rest))
                            },
                            _ => Err(ProtocolChoiceError::UnexpectedMessage),
                        }
                    })
        })
    })
}

/// Helps selecting a protocol amongst the ones supported.
///
/// Same as `dialer_select_proto`. Queries the list of supported protocols from the remote, then
/// chooses the most appropriate one.
pub fn dialer_select_proto_parallel<R, I, M, P>(
    inner: R,
    protocols: I,
) -> impl Future<Item = (P, R), Error = ProtocolChoiceError>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item = (Bytes, M, P)>,
    M: FnMut(&Bytes, &Bytes) -> bool,
{
    Dialer::new(inner)
        .from_err()
        .and_then(move |dialer| {
            trace!("requesting protocols list");
            dialer
                .send(DialerToListenerMessage::ProtocolsListRequest)
                .from_err()
        })
        .and_then(move |dialer| dialer.into_future().map_err(|(e, _)| e.into()))
        .and_then(move |(msg, dialer)| {
            trace!("protocols list response: {:?}", msg);
            let list = match msg {
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
            Ok((proto_name, proto_val, dialer))
        })
        .and_then(|(proto_name, proto_val, dialer)| {
            trace!("sending {:?}", proto_name);
            dialer
                .send(DialerToListenerMessage::ProtocolRequest {
                    name: proto_name.clone(),
                })
                .from_err()
                .map(|dialer| (proto_name, proto_val, dialer))
        })
        .and_then(|(proto_name, proto_val, dialer)| {
            dialer
                .into_future()
                .map(|(msg, rest)| (proto_name, proto_val, msg, rest))
                .map_err(|(err, _)| err.into())
        })
        .and_then(|(proto_name, proto_val, msg, dialer)| {
            trace!("received {:?}", msg);
            match msg {
                Some(ListenerToDialerMessage::ProtocolAck { ref name }) if name == &proto_name => {
                    Ok((proto_val, dialer.into_inner()))
                }
                _ => Err(ProtocolChoiceError::UnexpectedMessage),
            }
        })
}
