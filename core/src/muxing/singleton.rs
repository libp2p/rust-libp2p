// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::{
    connection::Endpoint,
    muxing::{OpenFlags, StreamMuxer, StreamMuxerEvent},
};

use futures::prelude::*;
use std::cell::Cell;
use std::{io, task::Context, task::Poll};

/// Implementation of `StreamMuxer` that allows only one substream on top of a connection,
/// yielding the connection itself.
///
/// Applying this muxer on a connection doesn't read or write any data on the connection itself.
/// Most notably, no protocol is negotiated.
pub struct SingletonMuxer<TSocket> {
    /// The inner connection.
    inner: Cell<Option<TSocket>>,
    /// Our local endpoint. Always the same value as was passed to `new`.
    endpoint: Endpoint,
}

impl<TSocket> SingletonMuxer<TSocket> {
    /// Creates a new `SingletonMuxer`.
    ///
    /// If `endpoint` is `Dialer`, then only one outbound substream will be permitted.
    /// If `endpoint` is `Listener`, then only one inbound substream will be permitted.
    pub fn new(inner: TSocket, endpoint: Endpoint) -> Self {
        SingletonMuxer {
            inner: Cell::new(Some(inner)),
            endpoint,
        }
    }
}

impl<TSocket> StreamMuxer for SingletonMuxer<TSocket>
where
    TSocket: AsyncRead + AsyncWrite + Unpin,
{
    type Substream = TSocket;
    type Error = io::Error;

    fn poll_event(
        &self,
        flags: OpenFlags,
        _: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent<Self::Substream>, Self::Error>> {
        // these combinations will never emit a stream.
        match self.endpoint {
            Endpoint::Dialer if flags == OpenFlags::INBOUND => return Poll::Pending,
            Endpoint::Listener if flags == OpenFlags::OUTBOUND => return Poll::Pending,
            _ => {}
        }

        // can only emit the stream once
        let socket = match self.inner.replace(None) {
            None => return Poll::Pending,
            Some(stream) => stream,
        };

        match self.endpoint {
            Endpoint::Dialer => Poll::Ready(Ok(StreamMuxerEvent::OutboundSubstream(socket))),
            Endpoint::Listener => Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(socket))),
        }
    }

    fn poll_close(&self, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}
