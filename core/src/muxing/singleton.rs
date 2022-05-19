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

use crate::muxing::OutboundSubstreamId;
use crate::{
    connection::Endpoint,
    muxing::{StreamMuxer, StreamMuxerEvent},
};

use futures::prelude::*;
use std::{io, task::Context, task::Poll};

/// Implementation of `StreamMuxer` that allows only one substream on top of a connection,
/// yielding the connection itself.
///
/// Applying this muxer on a connection doesn't read or write any data on the connection itself.
/// Most notably, no protocol is negotiated.
pub struct SingletonMuxer<TSocket> {
    /// The inner connection.
    inner: Option<TSocket>,
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
            inner: Some(inner),
            endpoint,
        }
    }
}

impl<TSocket> StreamMuxer for SingletonMuxer<TSocket>
where
    TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Substream = TSocket;

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<io::Result<StreamMuxerEvent<Self::Substream>>> {
        let stream = match self.inner.take() {
            None => return Poll::Pending,
            Some(stream) => stream,
        };

        let event = match self.endpoint {
            Endpoint::Dialer => {
                StreamMuxerEvent::OutboundSubstream(stream, OutboundSubstreamId::ZERO)
            }
            Endpoint::Listener => StreamMuxerEvent::InboundSubstream(stream),
        };

        Poll::Ready(Ok(event))
    }

    fn open_outbound(&mut self) -> OutboundSubstreamId {
        OutboundSubstreamId::ZERO
    }

    fn poll_close(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
