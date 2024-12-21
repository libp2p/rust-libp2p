use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{FutureExt, ready};
use futures::future::BoxFuture;
use wtransport::{error::ConnectionError, RecvStream, SendStream};

pub use connecting::Connecting;
use libp2p_core::muxing::StreamMuxerEvent;
use libp2p_core::StreamMuxer;

pub(crate) use crate::connection::stream::Stream;
use crate::Error;

mod connecting;
mod stream;

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

    fn poll_inbound(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
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

    fn poll_outbound(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        todo!()
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        todo!()
    }
}