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

use crate::{
    connection::{Connection, ConnectionEvent},
    muxer::QuicMuxer,
    transport,
};

use futures::prelude::*;
use libp2p_core::PeerId;
use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

/// A QUIC connection currently being negotiated.
pub struct Upgrade {
    connection: Option<Connection>,
}

impl Upgrade {
    /// Builds an [`Upgrade`] that wraps around a [`Connection`].
    pub(crate) fn from_connection(connection: Connection) -> Self {
        Upgrade {
            connection: Some(connection),
        }
    }
}

impl Future for Upgrade {
    type Output = Result<(PeerId, QuicMuxer), transport::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let connection = self
            .connection
            .as_mut()
            .expect("Future polled after it has completed");

        let event = Connection::poll_event(connection, cx);
        match event {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(ConnectionEvent::Connected) => {
                let peer_id = connection.remote_peer_id();
                let muxer = QuicMuxer::from_connection(self.connection.take().unwrap());
                return Poll::Ready(Ok((peer_id, muxer)));
            }
            Poll::Ready(ConnectionEvent::ConnectionLost(err)) => {
                return Poll::Ready(Err(transport::Error::Established(err)));
            }
            // Other items are:
            // - StreamAvailable
            // - StreamOpened
            // - StreamReadable
            // - StreamWritable
            // - StreamFinished
            // - StreamStopped
            Poll::Ready(_) => {
                // They can happen only after we finished handshake and connected to the peer.
                // But for `Upgrade` we get `Connected` event, wrap connection into a muxer
                // and pass it to the result Stream of muxers.
                unreachable!()
            }
        }
    }
}

impl fmt::Debug for Upgrade {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.connection, f)
    }
}
