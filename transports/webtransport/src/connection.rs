use std::pin::Pin;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use futures::FutureExt;
use wtransport::{error::ConnectionError, RecvStream, SendStream};

pub use connecting::Connecting;
use libp2p_core::muxing::StreamMuxerEvent;
use libp2p_core::StreamMuxer;
use libp2p_identity::PeerId;

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


    /// Returns the address of the node we're connected to.
    /// Panics if the connection is still handshaking.
    fn remote_peer_id(&self) -> PeerId {
        let cert = self.connection
            .peer_identity()
            .expect("connection got identity because it passed TLS handshake; qed")
            .as_slice().first().expect("there should be exactly one certificate; qed");

        let p2p_cert = libp2p_tls::certificate::parse_binary(cert.der())
            .expect("the certificate was validated during TLS handshake; qed");

        p2p_cert.peer_id()
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

        let (send, recv) = futures::ready!(incoming.poll_unpin(cx))?;
            // .map_err(ConnectionError)?;
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