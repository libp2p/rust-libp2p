// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

//! Future that drives a QUIC connection until is has performed its TLS handshake.

use crate::{connection::ConnectionEvent, muxer::Inner, Error, Muxer};

use futures::prelude::*;
use futures_timer::Delay;
use libp2p_core::PeerId;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

/// A QUIC connection currently being negotiated.
#[derive(Debug)]
pub struct Connecting {
    inner: Option<Inner>,
    timeout: Delay,
}

impl Connecting {
    /// Builds an [`Connecting`] that wraps around a [`Connection`].
    pub(crate) fn new(inner: Inner, timeout: Duration) -> Self {
        Connecting {
            inner: Some(inner),
            timeout: Delay::new(timeout),
        }
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, Muxer), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self
            .inner
            .as_mut()
            .expect("Future polled after it has completed");

        loop {
            match inner.connection.poll_event(cx) {
                Poll::Ready(ConnectionEvent::Connected(peer_id)) => {
                    let muxer = Muxer::new(self.inner.take().unwrap());
                    return Poll::Ready(Ok((peer_id, muxer)));
                }
                Poll::Ready(ConnectionEvent::ConnectionLost(err)) => return Poll::Ready(Err(err)),
                Poll::Ready(ConnectionEvent::HandshakeDataReady)
                | Poll::Ready(ConnectionEvent::StreamAvailable)
                | Poll::Ready(ConnectionEvent::StreamOpened)
                | Poll::Ready(ConnectionEvent::StreamReadable(_))
                | Poll::Ready(ConnectionEvent::StreamWritable(_))
                | Poll::Ready(ConnectionEvent::StreamFinished(_))
                | Poll::Ready(ConnectionEvent::StreamStopped(_)) => continue,
                Poll::Pending => {}
            }
            match self.timeout.poll_unpin(cx) {
                Poll::Ready(()) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
                Poll::Pending => {}
            }
            return Poll::Pending;
        }
    }
}
