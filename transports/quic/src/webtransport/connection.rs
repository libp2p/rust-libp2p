// Copyright 2020 Parity Technologies (UK) Ltd.
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

use h3_quinn::Connection as Http3Connection;
use h3_webtransport::server::WebTransportSession;
use bytes::Bytes;
use crate::Error;
use futures::{future::BoxFuture, FutureExt};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use std::sync::Arc;
use h3::quic::BidiStream;
use libp2p_core::upgrade::InboundConnectionUpgrade;
use libp2p_noise::Output;
use crate::webtransport::stream::Stream;

/// State for a single opened WebTransport session.
pub(crate) struct Connection {
    /// Underlying connection.
    session: Arc<WebTransportSession<Http3Connection, Bytes>>,
    //Noise config to auth incoming connections.
    noise: libp2p_noise::Config,
    /// Future for accepting a new incoming bidirectional stream.
    incoming: Option<
        BoxFuture<'static, Output<Stream>>,
    >,
    /// Future to wait for the connection to be closed.
    closing: Option<BoxFuture<'static, h3::Error>>,
}

impl Connection {
    /// Build a [`Connection`] from raw components.
    ///
    /// This function assumes that the [`quinn::Connection`] is completely fresh and none of
    /// its methods has ever been called. Failure to comply might lead to logic errors and panics.
    pub(crate) fn new(
        session: WebTransportSession<Http3Connection, Bytes>,
        noise: libp2p_noise::Config,
    ) -> Self {
        Self {
            session: Arc::new(session),
            noise,
            incoming: None,
            closing: None,
        }
    }
}

impl StreamMuxer for Connection {
    type Substream = Output<Stream>;
    type Error = Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.get_mut();
        let t_session = Arc::clone(&this.session);
        let t_noise = this.noise.clone();
        let incoming = this.incoming.get_or_insert_with(|| {
            async move {
                let res = t_session.accept_bi().await.unwrap();
                match res {
                    Some(h3_webtransport::server::AcceptedBi::BidiStream(_, stream)) => {
                        let (send, recv) = stream.split();
                        let stream = Stream::new(send, recv);

                        // todo should we apply `handshake_timeout` here?
                        let (_peer_id, out) = t_noise.upgrade_inbound(stream, "").await.unwrap();

                        out
                    }
                    _ => unreachable!("fix me!")
                }
            }.boxed()
        });

        let res = futures::ready!(incoming.poll_unpin(cx));
        this.incoming.take();
        Poll::Ready(Ok(res))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        panic!("WebTransport implementation doesn't support outbound streams.")
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        // TODO: If connection migration is enabled (currently disabled) address
        // change on the connection needs to be handled.
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        /*let this = self.get_mut();

        let closing = this.closing.get_or_insert_with(|| {
            this.session.
            // this.connection.close(From::from(0u32), &[]);
            // let connection = this.connection.clone();
            async move { connection.closed().await }.boxed()
        });

        match futures::ready!(closing.poll_unpin(cx)) {
            // Expected error given that `connection.close` was called above.
            quinn::ConnectionError::LocallyClosed => {}
            error => return Poll::Ready(Err(Error::Connection(ConnectionError(error)))),
        };*/

        Poll::Ready(Ok(()))
    }
}