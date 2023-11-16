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
use h3::quic::BidiStream;
use crate::webtransport::stream::Stream;

/// State for a single opened WebTransport session.
pub struct Connection {
    /// Underlying connection.
    session: WebTransportSession<Http3Connection, Bytes>,
    /// Future for accepting a new incoming bidirectional stream.
    incoming: Option<
        BoxFuture<'static, (h3_webtransport::stream::SendStream<h3_quinn::SendStream<Bytes>, Bytes>,
                            h3_webtransport::stream::RecvStream<h3_quinn::RecvStream, Bytes>)>,
    >,
    /// Future for opening a new outgoing bidirectional stream.
    outgoing: Option<
        BoxFuture<'static, (h3_webtransport::stream::SendStream<h3_quinn::SendStream<Bytes>, Bytes>,
                            h3_webtransport::stream::RecvStream<h3_quinn::RecvStream, Bytes>)>,
    >,
    /// Future to wait for the connection to be closed.
    closing: Option<BoxFuture<'static, h3::Error>>,
}

impl Connection {
    /// Build a [`Connection`] from raw components.
    ///
    /// This function assumes that the [`quinn::Connection`] is completely fresh and none of
    /// its methods has ever been called. Failure to comply might lead to logic errors and panics.
    pub(crate) fn new(session: WebTransportSession<Http3Connection, Bytes>) -> Self {
        Self {
            session,
            incoming: None,
            outgoing: None,
            closing: None,
        }
    }
}

impl StreamMuxer for Connection {
    type Substream = Stream;
    type Error = Error;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.get_mut();

        let incoming = this.incoming.get_or_insert_with(|| {
            let accept = this.session.accept_bi();
            async move {
                let res = accept.await.unwrap();
                match res {
                    Some(h3_webtransport::server::AcceptedBi::BidiStream(_, stream)) => {
                        stream.split()
                    }
                    _ => unreachable!("fix me!")
                }
            }.boxed()
        });

        let (send, recv) = futures::ready!(incoming.poll_unpin(cx));
        this.incoming.take();
        let stream = Stream::new(send, recv);
        Poll::Ready(Ok(stream))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.get_mut();

        let session_id = this.session.session_id();
        let open_bi = this.session.open_bi(session_id);

        let outgoing = this.outgoing.get_or_insert_with(|| {
            async move {
                // let stream = open_bi.await.unwrap();
                //
                // stream.split()
                unreachable!("fix me!")
            }.boxed()
        });

        let (send, recv) = futures::ready!(outgoing.poll_unpin(cx));
        this.outgoing.take();
        let stream = Stream::new(send, recv);
        Poll::Ready(Ok(stream))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        // TODO: If connection migration is enabled (currently disabled) address
        // change on the connection needs to be handled.
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
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