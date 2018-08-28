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

//! Contains the `Dialer` wrapper, which allows raw communications with a listener.

use bytes::{Bytes, BytesMut};
use futures::{Async, AsyncSink, Future, Poll, Sink, StartSend, Stream};
use length_delimited::LengthDelimitedFramedRead;
use protocol::DialerToListenerMessage;
use protocol::ListenerToDialerMessage;
use protocol::MultistreamSelectError;
use protocol::MULTISTREAM_PROTOCOL_WITH_LF;
use std::io::{BufRead, Cursor};
use tokio_io::codec::length_delimited::Builder as LengthDelimitedBuilder;
use tokio_io::codec::length_delimited::FramedWrite as LengthDelimitedFramedWrite;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::decode;

/// Wraps around a `AsyncRead+AsyncWrite`. Assumes that we're on the dialer's side. Produces and
/// accepts messages.
pub struct Dialer<R> {
    inner: LengthDelimitedFramedRead<Bytes, LengthDelimitedFramedWrite<R, BytesMut>>,
    handshake_finished: bool,
}

impl<R> Dialer<R>
where
    R: AsyncRead + AsyncWrite,
{
    /// Takes ownership of a socket and starts the handshake. If the handshake succeeds, the
    /// future returns a `Dialer`.
    pub fn new(inner: R) -> impl Future<Item = Dialer<R>, Error = MultistreamSelectError> {
        let write = LengthDelimitedBuilder::new()
            .length_field_length(1)
            .new_write(inner);
        let inner = LengthDelimitedFramedRead::new(write);

        inner
            .send(BytesMut::from(MULTISTREAM_PROTOCOL_WITH_LF))
            .from_err()
            .map(|inner| Dialer {
                inner,
                handshake_finished: false,
            })
    }

    /// Grants back the socket. Typically used after a `ProtocolAck` has been received.
    #[inline]
    pub fn into_inner(self) -> R {
        self.inner.into_inner().into_inner()
    }
}

impl<R> Sink for Dialer<R>
where
    R: AsyncRead + AsyncWrite,
{
    type SinkItem = DialerToListenerMessage;
    type SinkError = MultistreamSelectError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match item {
            DialerToListenerMessage::ProtocolRequest { name } => {
                if !name.starts_with(b"/") {
                    return Err(MultistreamSelectError::WrongProtocolName);
                }
                let mut protocol = BytesMut::from(name);
                protocol.extend_from_slice(&[b'\n']);
                match self.inner.start_send(protocol) {
                    Ok(AsyncSink::Ready) => Ok(AsyncSink::Ready),
                    Ok(AsyncSink::NotReady(mut protocol)) => {
                        let protocol_len = protocol.len();
                        protocol.truncate(protocol_len - 1);
                        let protocol = protocol.freeze();
                        Ok(AsyncSink::NotReady(
                            DialerToListenerMessage::ProtocolRequest { name: protocol },
                        ))
                    }
                    Err(err) => Err(err.into()),
                }
            }

            DialerToListenerMessage::ProtocolsListRequest => {
                match self.inner.start_send(BytesMut::from(&b"ls\n"[..])) {
                    Ok(AsyncSink::Ready) => Ok(AsyncSink::Ready),
                    Ok(AsyncSink::NotReady(_)) => Ok(AsyncSink::NotReady(
                        DialerToListenerMessage::ProtocolsListRequest,
                    )),
                    Err(err) => Err(err.into()),
                }
            }
        }
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.inner.poll_complete()?)
    }
}

impl<R> Stream for Dialer<R>
where
    R: AsyncRead + AsyncWrite,
{
    type Item = ListenerToDialerMessage;
    type Error = MultistreamSelectError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let mut frame = match self.inner.poll() {
                Ok(Async::Ready(Some(frame))) => frame,
                Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(err) => return Err(err.into()),
            };

            if !self.handshake_finished {
                if frame == MULTISTREAM_PROTOCOL_WITH_LF {
                    self.handshake_finished = true;
                    continue;
                } else {
                    return Err(MultistreamSelectError::FailedHandshake);
                }
            }

            if frame.get(0) == Some(&b'/') && frame.last() == Some(&b'\n') {
                let frame_len = frame.len();
                let protocol = frame.split_to(frame_len - 1);
                return Ok(Async::Ready(Some(ListenerToDialerMessage::ProtocolAck {
                    name: protocol,
                })));
            } else if frame == b"na\n"[..] {
                return Ok(Async::Ready(Some(ListenerToDialerMessage::NotAvailable)));
            } else {
                // A varint number of protocols
                let (num_protocols, remaining) = decode::usize(&frame)?;
                let reader = Cursor::new(remaining);
                let mut iter = BufRead::split(reader, b'\r');
                if !iter.next()
                    .ok_or(MultistreamSelectError::UnknownMessage)??
                    .is_empty()
                {
                    return Err(MultistreamSelectError::UnknownMessage);
                }

                let mut out = Vec::with_capacity(num_protocols);
                for proto in iter.by_ref().take(num_protocols) {
                    let mut proto = proto?;
                    let poped = proto.pop(); // Pop the `\n`
                    if poped != Some(b'\n') {
                        return Err(MultistreamSelectError::UnknownMessage);
                    }
                    out.push(Bytes::from(proto));
                }

                // Making sure that the number of protocols was correct.
                if iter.next().is_some() || out.len() != num_protocols {
                    return Err(MultistreamSelectError::UnknownMessage);
                }

                return Ok(Async::Ready(Some(
                    ListenerToDialerMessage::ProtocolsListResponse { list: out },
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate tokio_current_thread;
    extern crate tokio_tcp;
    use self::tokio_tcp::{TcpListener, TcpStream};
    use bytes::Bytes;
    use futures::Future;
    use futures::{Sink, Stream};
    use protocol::{Dialer, DialerToListenerMessage, MultistreamSelectError};

    #[test]
    fn wrong_proto_name() {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map(|_| ())
            .map_err(|(e, _)| e.into());

        let client = TcpStream::connect(&listener_addr)
            .from_err()
            .and_then(move |stream| Dialer::new(stream))
            .and_then(move |dialer| {
                let p = Bytes::from("invalid_name");
                dialer.send(DialerToListenerMessage::ProtocolRequest { name: p })
            });

        match tokio_current_thread::block_on_all(server.join(client)) {
            Err(MultistreamSelectError::WrongProtocolName) => (),
            _ => panic!(),
        }
    }
}
