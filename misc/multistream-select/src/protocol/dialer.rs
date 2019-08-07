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

use super::*;

use bytes::{Bytes, BytesMut};
use crate::length_delimited::LengthDelimited;
use crate::protocol::{Request, Response, MultistreamSelectError};
use futures::{prelude::*, sink, Async, StartSend, try_ready};
use tokio_io::{AsyncRead, AsyncWrite};
use std::marker;
use unsigned_varint as uvi;

/// The maximum number of supported protocols that can be processed.
const MAX_PROTOCOLS: usize = 1000;

/// Wraps around a `AsyncRead+AsyncWrite`.
/// Assumes that we're on the dialer's side. Produces and accepts messages.
pub struct Dialer<R, N> {
    inner: LengthDelimited<R>,
    handshake_finished: bool,
    _protocol_name: marker::PhantomData<N>,
}

impl<R, N> Dialer<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    pub fn dial(inner: R) -> DialerFuture<R, N> {
        let io = LengthDelimited::new(inner);
        let mut buf = BytesMut::new();
        Header::Multistream10.encode(&mut buf);
        DialerFuture {
            inner: io.send(buf.freeze()),
            _protocol_name: marker::PhantomData,
        }
    }

    /// Grants back the socket. Typically used after a `ProtocolAck` has been received.
    pub fn into_inner(self) -> R {
        self.inner.into_inner()
    }
}

impl<R, N> Sink for Dialer<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    type SinkItem = Request<N>;
    type SinkError = MultistreamSelectError;

    fn start_send(&mut self, request: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let mut msg = BytesMut::new();
        request.encode(&mut msg)?;
        match self.inner.start_send(msg.freeze())? {
            AsyncSink::NotReady(_) => Ok(AsyncSink::NotReady(request)),
            AsyncSink::Ready => Ok(AsyncSink::Ready),
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.inner.poll_complete()?)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.inner.close()?)
    }
}

impl<R, N> Stream for Dialer<R, N>
where
    R: AsyncRead + AsyncWrite
{
    type Item = Response<Bytes>;
    type Error = MultistreamSelectError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let mut msg = match self.inner.poll() {
                Ok(Async::Ready(Some(msg))) => msg,
                Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(err) => return Err(err.into()),
            };

            if !self.handshake_finished {
                if msg == MSG_MULTISTREAM_1_0 {
                    self.handshake_finished = true;
                    continue;
                } else {
                    return Err(MultistreamSelectError::FailedHandshake);
                }
            }

            if msg.get(0) == Some(&b'/') && msg.last() == Some(&b'\n') {
                let len = msg.len();
                let name = msg.split_to(len - 1);
                return Ok(Async::Ready(Some(
                    Response::Protocol { name }
                )));
            } else if msg == MSG_PROTOCOL_NA {
                return Ok(Async::Ready(Some(Response::ProtocolNotAvailable)));
            } else {
                // A varint number of protocols
                let (num_protocols, mut remaining) = uvi::decode::usize(&msg)?;
                if num_protocols > MAX_PROTOCOLS { // TODO: configurable limit
                    return Err(MultistreamSelectError::TooManyProtocols)
                }
                let mut protocols = Vec::with_capacity(num_protocols);
                for _ in 0 .. num_protocols {
                    let (len, rem) = uvi::decode::usize(remaining)?;
                    if len == 0 || len > rem.len() || rem[len - 1] != b'\n' {
                        return Err(MultistreamSelectError::UnknownMessage)
                    }
                    protocols.push(Bytes::from(&rem[.. len - 1]));
                    remaining = &rem[len ..]
                }
                return Ok(Async::Ready(Some(
                    Response::SupportedProtocols { protocols },
                )));
            }
        }
    }
}

/// Future, returned by `Dialer::new`, which send the handshake and returns the actual `Dialer`.
pub struct DialerFuture<T: AsyncWrite, N: AsRef<[u8]>> {
    inner: sink::Send<LengthDelimited<T>>,
    _protocol_name: marker::PhantomData<N>,
}

impl<T: AsyncWrite, N: AsRef<[u8]>> Future for DialerFuture<T, N> {
    type Item = Dialer<T, N>;
    type Error = MultistreamSelectError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let inner = try_ready!(self.inner.poll());
        Ok(Async::Ready(Dialer {
            inner,
            handshake_finished: false,
            _protocol_name: marker::PhantomData,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::current_thread::Runtime;
    use tokio_tcp::{TcpListener, TcpStream};
    use futures::Future;
    use futures::{Sink, Stream};

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
            .and_then(move |stream| Dialer::dial(stream))
            .and_then(move |dialer| {
                let name = b"invalid_name";
                dialer.send(Request::Protocol { name })
            });

        let mut rt = Runtime::new().unwrap();
        match rt.block_on(server.join(client)) {
            Err(MultistreamSelectError::InvalidProtocolName) => (),
            _ => panic!(),
        }
    }
}
