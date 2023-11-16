//! Future that drives a QUIC connection until is has performed its TLS handshake.
//! And then it drives a http3 connection based on the QUIC connection.
//! And then it drives a WebTransport session based on the http3 connection.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use futures::{
    future::{select, Select},
    prelude::*,
};
use futures::future::{BoxFuture, Either};
use futures_timer::Delay;
use h3::ext::Protocol;
use h3_quinn::Connection as Http3Connection;
use h3_webtransport::server::WebTransportSession;
use http::Method;

use libp2p_core::muxing::StreamMuxerBox;
use libp2p_identity::PeerId;

use crate::{Error, webtransport};

// #[derive(Debug)]
pub struct Connecting {
    connecting: Select<
        BoxFuture<'static, Result<(PeerId, WebTransportSession<Http3Connection, Bytes>), WebTransportError>>,
        Delay
    >,
}

impl Connecting {
    pub(crate) fn new(connecting: quinn::Connecting, timeout: Duration) -> Self {
        Connecting {
            connecting: select(web_transport_session(connecting).boxed(), Delay::new(timeout))
        }
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, StreamMuxerBox), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = match futures::ready!(self.connecting.poll_unpin(cx)) {
            Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
            Either::Left((Ok((peer_id, session)), _)) => {
                let muxer = webtransport::Connection::new(session);
                Ok((peer_id, StreamMuxerBox::new(muxer)))
            }
            Either::Left((Err(_e), _)) => return Poll::Ready(Err(Error::WebTransportError)),
        };

        Poll::Ready(res)
    }
}

fn remote_peer_id(connection: &quinn::Connection) -> PeerId {
    let identity = connection
        .peer_identity()
        .expect("connection got identity because it passed TLS handshake; qed");
    let certificates: Box<Vec<rustls::Certificate>> =
        identity.downcast().expect("we rely on rustls feature; qed");
    let end_entity = certificates
        .first()
        .expect("there should be exactly one certificate; qed");
    let p2p_cert = libp2p_tls::certificate::parse(end_entity)
        .expect("the certificate was validated during TLS handshake; qed");
    p2p_cert.peer_id()
}

async fn web_transport_session(connecting: quinn::Connecting)
                               -> Result<(PeerId, WebTransportSession<Http3Connection, Bytes>), WebTransportError> {
    let quic_conn = connecting.await
        .map_err(|e| WebTransportError::ConnectionError(e))?;
    // info!("new http3 established");

    let peer_id = remote_peer_id(&quic_conn);

    let mut h3_conn = h3::server::builder()
        .enable_webtransport(true)
        .enable_connect(true)
        .enable_datagram(true)
        .max_webtransport_sessions(1)
        .send_grease(true)
        .build(h3_quinn::Connection::new(quic_conn))
        .await
        .unwrap();

    match h3_conn.accept().await {
        Ok(Some((request, stream))) => {
            let ext = request.extensions();
            let proto = ext.get::<Protocol>();
            if Some(&Protocol::WEB_TRANSPORT) == proto {
                if request.method() == &Method::CONNECT {
                    let session = WebTransportSession::accept(request, stream, h3_conn)
                        .await.map_err(|e| WebTransportError::Http3Error(e))?;
                    Ok((peer_id, session))
                } else {
                    Err(WebTransportError::UnexpectedMethod(request.method().clone()))
                }
            } else {
                Err(WebTransportError::UnexpectedProtocol(proto.cloned()))
            }
        }
        Ok(None) => {
            // indicating no more streams to be received
            Err(WebTransportError::ClosedConnection)
        }
        Err(err) => {
            Err(WebTransportError::Http3Error(err))
        }
    }
}

// #[derive(Debug)]
pub enum WebTransportError {
    ConnectionError(quinn::ConnectionError),
    UnexpectedMethod(Method),
    UnexpectedProtocol(Option<Protocol>),
    ClosedConnection,
    Http3Error(h3::Error),
}