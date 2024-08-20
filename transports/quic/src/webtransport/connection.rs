use std::fmt::{Display, Formatter};
use std::io;
use std::io::ErrorKind;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::{AsyncRead, AsyncWrite, FutureExt};
use futures::future::BoxFuture;
use h3::quic;
use h3_webtransport::server::{AcceptedBi, WebTransportSession};
use h3_webtransport::stream::{RecvStream, SendStream};

use libp2p_core::muxing::StreamMuxerEvent;
use libp2p_core::StreamMuxer;

use crate::{ConnectionError, Error};

type SendPart = SendStream<h3_quinn::SendStream<Bytes>, Bytes>;
type RecvPart = RecvStream<h3_quinn::RecvStream, Bytes>;

pub(crate) struct Connection {
    session: Arc<WebTransportSession<h3_quinn::Connection, Bytes>>,
    incoming: Option<BoxFuture<'static, Result<Stream, Error>>>,
    /// Underlying connection.
    connection: quinn::Connection,
    /// Future to wait for the connection to be closed.
    closing: Option<BoxFuture<'static, quinn::ConnectionError>>,
}

impl Connection {
    pub(crate) fn new(
        session: Arc<WebTransportSession<h3_quinn::Connection, Bytes>>,
        connection: quinn::Connection
    ) -> Self {
        Self {
            session,
            incoming: None,
            connection,
            closing: None,
        }
    }
}

impl StreamMuxer for Connection {
    type Substream = Stream;
    type Error = Error;

    fn poll_inbound(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        let this = self.get_mut();
        let t_session = Arc::clone(&this.session);

        let inbound = this.incoming.get_or_insert_with(|| {
            async move { accept_webtransport_stream(&t_session).await }.boxed()
        });

        let res = futures::ready!(inbound.poll_unpin(cx))?;
        this.incoming.take();
        Poll::Ready(Ok(res))
    }

    fn poll_outbound(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        panic!("WebTransport implementation doesn't support outbound streams.")
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();

        let closing = this.closing.get_or_insert_with(|| {
            this.connection.close(From::from(0u32), &[]);
            let connection = this.connection.clone();
            async move { connection.closed().await }.boxed()
        });

        match futures::ready!(closing.poll_unpin(cx)) {
            // Expected error given that `connection.close` was called above.
            quinn::ConnectionError::LocallyClosed => {}
            error => return Poll::Ready(Err(Error::Connection(ConnectionError(error)))),
        };

        Poll::Ready(Ok(()))
    }

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        // TODO: If connection migration is enabled (currently disabled) address
        // change on the connection needs to be handled.
        Poll::Pending
    }
}

/// A single stream on a connection
pub(crate) struct Stream {
    send: SendPart,
    recv: RecvPart,
    /// Whether the stream is closed or not
    close_result: Option<Result<(), io::ErrorKind>>,
}

impl Stream {
    pub(crate) fn new(send: SendPart, recv: RecvPart) -> Self {
        Self { send, recv, close_result: None }
    }
}

impl AsyncRead for Stream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        if let Some(close_result) = self.close_result {
            if close_result.is_err() {
                return Poll::Ready(Ok(0));
            }
        }

        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.send)
            .poll_write(cx, buf)
            .map_err(Into::into)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        if let Some(close_result) = self.close_result {
            // For some reason poll_close needs to be 'fuse'able
            return Poll::Ready(close_result.map_err(Into::into));
        }
        let close_result = futures::ready!(Pin::new(&mut self.send).poll_close(cx));
        self.close_result = Some(close_result.as_ref().map_err(|e| e.kind()).copied());
        Poll::Ready(close_result)
    }
}

#[derive(Debug)]
pub struct WebtransportError(String);

impl Display for WebtransportError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for WebtransportError {}

impl From<WebtransportError> for Error {
    fn from(value: WebtransportError) -> Self {
        Error::Io(io::Error::new(ErrorKind::Other, value))
    }
}

pub(crate) async fn accept_webtransport_stream(
    session: &Arc<WebTransportSession<h3_quinn::Connection, Bytes>>,
) -> Result<Stream, Error> {
    let t_session = Arc::clone(session);

    match t_session.accept_bi().await {
        Ok(Some(AcceptedBi::BidiStream(_, stream))) => {
            let (send, recv) = quic::BidiStream::split(stream);
            Ok(Stream::new(send, recv))
        }
        Ok(_) => Err(
            WebtransportError(String::from("Only bidirectional streams are supported")).into()
        ),
        Err(e) => Err(Error::Io(io::Error::new(ErrorKind::Other, e))),
    }
}