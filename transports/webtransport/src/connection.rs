use std::pin::Pin;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use futures::{ready, FutureExt};
use wtransport::{error::ConnectionError, RecvStream, SendStream};

pub(crate) use connecting::Connecting;
use libp2p_core::muxing::StreamMuxerEvent;
use libp2p_core::StreamMuxer;

pub(crate) use crate::connection::stream::Stream;
use crate::Error;

mod connecting;
mod stream;

/// State for a single opened Webtransport connection.
pub struct Connection {
    /// Underlying connection.
    connection: wtransport::Connection,
    /// Future for accepting a new incoming bidirectional stream.
    incoming: Option<BoxFuture<'static, Result<(SendStream, RecvStream), ConnectionError>>>,
    /// Future to wait for the connection to be closed.
    closing: Option<BoxFuture<'static, ConnectionError>>,
}

impl Connection {
    fn new(connection: wtransport::Connection) -> Self {
        Self {
            connection,
            incoming: None,
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
            let connection = this.connection.clone();
            async move { connection.accept_bi().await }.boxed()
        });

        let (send, recv) = ready!(incoming.poll_unpin(cx))?;
        this.incoming.take();
        let stream = Stream::new(send, recv);
        Poll::Ready(Ok(stream))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();

        let closing = this.closing.get_or_insert_with(|| {
            this.connection.close(From::from(0u32), &[]);
            let connection = this.connection.clone();
            async move { connection.closed().await }.boxed()
        });

        match ready!(closing.poll_unpin(cx)) {
            // Expected error given that `connection.close` was called above.
            ConnectionError::LocallyClosed => {}
            error => return Poll::Ready(Err(Error::Connection(error))),
        };

        Poll::Ready(Ok(()))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        // Doesn't support outbound connections!
        Poll::Pending
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        // TODO: If connection migration is enabled (currently disabled) address
        // change on the connection needs to be handled.
        Poll::Pending
    }
}
