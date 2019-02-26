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

//! Contains the `Listener` wrapper, which allows raw communications with a dialer.

use bytes::{BufMut, Bytes, BytesMut};
use crate::length_delimited::LengthDelimited;
use crate::protocol::DialerToListenerMessage;
use crate::protocol::ListenerToDialerMessage;
use crate::protocol::MultistreamSelectError;
use crate::protocol::MULTISTREAM_PROTOCOL_WITH_LF;
use futures::{prelude::*, sink, stream::StreamFuture};
use log::{debug, trace};
use std::{io, mem};
use tokio_codec::Encoder;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::{encode, codec::Uvi};

/// Wraps around a `AsyncRead+AsyncWrite`. Assumes that we're on the listener's side. Produces and
/// accepts messages.
pub struct Listener<R, N> {
    inner: LengthDelimited<R, MessageEncoder<N>>
}

impl<R, N> Listener<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    /// Takes ownership of a socket and starts the handshake. If the handshake succeeds, the
    /// future returns a `Listener`.
    pub fn listen(inner: R) -> ListenerFuture<R, N> {
        let codec = MessageEncoder(std::marker::PhantomData);
        let inner = LengthDelimited::new(inner, codec);
        ListenerFuture {
            inner: ListenerFutureState::Await { inner: inner.into_future() }
        }
    }

    /// Grants back the socket. Typically used after a `ProtocolRequest` has been received and a
    /// `ProtocolAck` has been sent back.
    #[inline]
    pub fn into_inner(self) -> R {
        self.inner.into_inner()
    }
}

impl<R, N> Sink for Listener<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    type SinkItem = ListenerToDialerMessage<N>;
    type SinkError = MultistreamSelectError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match self.inner.start_send(Message::Body(item))? {
            AsyncSink::NotReady(Message::Body(item)) => Ok(AsyncSink::NotReady(item)),
            AsyncSink::NotReady(Message::Header) => unreachable!(),
            AsyncSink::Ready => Ok(AsyncSink::Ready)
        }
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.inner.poll_complete()?)
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.inner.close()?)
    }
}

impl<R, N> Stream for Listener<R, N>
where
    R: AsyncRead + AsyncWrite,
{
    type Item = DialerToListenerMessage<Bytes>;
    type Error = MultistreamSelectError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut frame = match self.inner.poll() {
            Ok(Async::Ready(Some(frame))) => frame,
            Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(err) => return Err(err.into()),
        };

        if frame.get(0) == Some(&b'/') && frame.last() == Some(&b'\n') {
            let frame_len = frame.len();
            let protocol = frame.split_to(frame_len - 1);
            Ok(Async::Ready(Some(
                DialerToListenerMessage::ProtocolRequest { name: protocol },
            )))
        } else if frame == b"ls\n"[..] {
            Ok(Async::Ready(Some(
                DialerToListenerMessage::ProtocolsListRequest,
            )))
        } else {
            Err(MultistreamSelectError::UnknownMessage)
        }
    }
}


/// Future, returned by `Listener::new` which performs the handshake and returns
/// the `Listener` if successful.
pub struct ListenerFuture<T: AsyncRead + AsyncWrite, N: AsRef<[u8]>> {
    inner: ListenerFutureState<T, N>
}

enum ListenerFutureState<T: AsyncRead + AsyncWrite, N: AsRef<[u8]>> {
    Await {
        inner: StreamFuture<LengthDelimited<T, MessageEncoder<N>>>
    },
    Reply {
        sender: sink::Send<LengthDelimited<T, MessageEncoder<N>>>
    },
    Undefined
}

impl<T: AsyncRead + AsyncWrite, N: AsRef<[u8]>> Future for ListenerFuture<T, N> {
    type Item = Listener<T, N>;
    type Error = MultistreamSelectError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, ListenerFutureState::Undefined) {
                ListenerFutureState::Await { mut inner } => {
                    let (msg, socket) =
                        match inner.poll() {
                            Ok(Async::Ready(x)) => x,
                            Ok(Async::NotReady) => {
                                self.inner = ListenerFutureState::Await { inner };
                                return Ok(Async::NotReady)
                            }
                            Err((e, _)) => return Err(MultistreamSelectError::from(e))
                        };
                    if msg.as_ref().map(|b| &b[..]) != Some(MULTISTREAM_PROTOCOL_WITH_LF) {
                        debug!("failed handshake; received: {:?}", msg);
                        return Err(MultistreamSelectError::FailedHandshake)
                    }
                    trace!("sending back /multistream/<version> to finish the handshake");
                    let sender = socket.send(Message::Header);
                    self.inner = ListenerFutureState::Reply { sender }
                }
                ListenerFutureState::Reply { mut sender } => {
                    let listener = match sender.poll()? {
                        Async::Ready(x) => x,
                        Async::NotReady => {
                            self.inner = ListenerFutureState::Reply { sender };
                            return Ok(Async::NotReady)
                        }
                    };
                    return Ok(Async::Ready(Listener { inner: listener }))
                }
                ListenerFutureState::Undefined => panic!("ListenerFutureState::poll called after completion")
            }
        }
    }
}

/// tokio-codec `Encoder` handling `ListenerToDialerMessage` values.
struct MessageEncoder<N>(std::marker::PhantomData<N>);

enum Message<N> {
    Header,
    Body(ListenerToDialerMessage<N>)
}

impl<N: AsRef<[u8]>> Encoder for MessageEncoder<N> {
    type Item = Message<N>;
    type Error = MultistreamSelectError;

    fn encode(&mut self, item: Self::Item, dest: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::Header => {
                Uvi::<usize>::default().encode(MULTISTREAM_PROTOCOL_WITH_LF.len(), dest)?;
                dest.reserve(MULTISTREAM_PROTOCOL_WITH_LF.len());
                dest.put(MULTISTREAM_PROTOCOL_WITH_LF);
                Ok(())
            }
            Message::Body(ListenerToDialerMessage::ProtocolAck { name }) => {
                if !name.as_ref().starts_with(b"/") {
                    return Err(MultistreamSelectError::WrongProtocolName)
                }
                let len = name.as_ref().len() + 1; // + 1 for \n
                if len > std::u16::MAX as usize {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "name too long").into())
                }
                Uvi::<usize>::default().encode(len, dest)?;
                dest.reserve(len);
                dest.put(name.as_ref());
                dest.put(&b"\n"[..]);
                Ok(())
            }
            Message::Body(ListenerToDialerMessage::ProtocolsListResponse { list }) => {
                let mut buf = encode::usize_buffer();
                let mut out_msg = Vec::from(encode::usize(list.len(), &mut buf));
                for e in &list {
                    if e.as_ref().len() + 1 > std::u16::MAX as usize {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "name too long").into())
                    }
                    out_msg.extend(encode::usize(e.as_ref().len() + 1, &mut buf)); // +1 for '\n'
                    out_msg.extend_from_slice(e.as_ref());
                    out_msg.push(b'\n')
                }
                let len = encode::usize(out_msg.len(), &mut buf);
                dest.reserve(len.len() + out_msg.len());
                dest.put(len);
                dest.put(out_msg);
                Ok(())
            }
            Message::Body(ListenerToDialerMessage::NotAvailable) => {
                Uvi::<usize>::default().encode(3, dest)?;
                dest.reserve(3);
                dest.put(&b"na\n"[..]);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::runtime::current_thread::Runtime;
    use tokio_tcp::{TcpListener, TcpStream};
    use bytes::Bytes;
    use futures::Future;
    use futures::{Sink, Stream};
    use crate::protocol::{Dialer, Listener, ListenerToDialerMessage, MultistreamSelectError};

    #[test]
    fn wrong_proto_name() {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map_err(|(e, _)| e.into())
            .and_then(move |(connec, _)| Listener::listen(connec.unwrap()))
            .and_then(|listener| {
                let proto_name = Bytes::from("invalid-proto");
                listener.send(ListenerToDialerMessage::ProtocolAck { name: proto_name })
            });

        let client = TcpStream::connect(&listener_addr)
            .from_err()
            .and_then(move |stream| Dialer::<_, Bytes>::dial(stream));

        let mut rt = Runtime::new().unwrap();
        match rt.block_on(server.join(client)) {
            Err(MultistreamSelectError::WrongProtocolName) => (),
            _ => panic!(),
        }
    }
}
