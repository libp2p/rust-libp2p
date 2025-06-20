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

use std::{
    collections::HashSet,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use bytes::Bytes;
use futures::{
    future::{select, BoxFuture, Either, FutureExt, Select},
    prelude::*,
};
use futures_timer::Delay;
use h3::ext::Protocol;
use h3_webtransport::server::WebTransportSession;
use http::Method;
use libp2p_core::{
    muxing::StreamMuxerBox, upgrade::InboundConnectionUpgrade,
};
use libp2p_identity::PeerId;
use quinn::rustls::pki_types::CertificateDer;
use crate::{transport::ConnectingMode, webtransport, webtransport::WebtransportConnectingError, Connection, ConnectionError, Error, CertHash};

/// A QUIC connection currently being negotiated.
pub struct Connecting {
    connecting: Select<BoxFuture<'static, Result<(PeerId, StreamMuxerBox), Error>>, Delay>,
}

impl Connecting {
    pub(crate) fn new(
        connecting: quinn::Connecting,
        mode: ConnectingMode,
        timeout: Duration,
    ) -> Self {
        Connecting {
            connecting: select(handshake(connecting, mode).boxed(), Delay::new(timeout)),
        }
    }
}

impl Connecting {
    /// Returns the address of the node we're connected to.
    /// Panics if the connection is still handshaking.
    fn remote_peer_id(connection: &quinn::Connection) -> PeerId {
        let identity = connection
            .peer_identity()
            .expect("connection got identity because it passed TLS handshake; qed");
        let certificates: Box<Vec<CertificateDer>> =
            identity.downcast().expect("we rely on rustls feature; qed");
        let end_entity = certificates
            .first()
            .expect("there should be exactly one certificate; qed");
        let p2p_cert = libp2p_tls::certificate::parse(end_entity)
            .expect("the certificate was validated during TLS handshake; qed");
        p2p_cert.peer_id()
    }
}

async fn handshake(
    connecting: quinn::Connecting,
    mode: ConnectingMode,
) -> Result<(PeerId, StreamMuxerBox), Error> {
    match connecting.await {
        Ok(connection) => {
            tracing::debug!("Got the incoming connection {:?}", connection);
            match mode {
                ConnectingMode::QUIC => {
                    let peer_id = Connecting::remote_peer_id(&connection);
                    let muxer = Connection::new(connection);
                    return Ok((peer_id, StreamMuxerBox::new(muxer)));
                }
                ConnectingMode::WebTransport(certhashes, noise_config) => {
                    let (peer_id, muxer) =
                        webtransport_connecting(connection, certhashes, noise_config)
                            .await?;
                    Ok((peer_id, StreamMuxerBox::new(muxer)))
                }
                ConnectingMode::Mixed(certhashes, noise_config) => {
                    let res = webtransport_connecting(
                        connection.clone(),
                        certhashes,
                        noise_config,
                    )
                    .await;
                    match res {
                        Ok((peer_id, muxer)) => Ok((peer_id, StreamMuxerBox::new(muxer))),
                        Err(_) => {
                            // todo What is the marker that it's should be regular quic connection?
                            let peer_id = Connecting::remote_peer_id(&connection);
                            let muxer = Connection::new(connection);
                            Ok((peer_id, StreamMuxerBox::new(muxer)))
                        },
                    }
                }
            }
        }
        Err(e) => {
            return Err(Error::Connection(ConnectionError(e)));
        }
    }
}

async fn webtransport_connecting(
    connection: quinn::Connection,
    certhashes: Vec<CertHash>,
    noise_config: libp2p_noise::Config,
) -> Result<(PeerId, StreamMuxerBox), WebtransportConnectingError> {
    let c_conn = connection.clone();
    let mut h3_conn: h3::server::Connection<h3_quinn::Connection, Bytes> = h3::server::builder()
        .enable_webtransport(true)
        .enable_extended_connect(true)
        .enable_datagram(true)
        .max_webtransport_sessions(5)
        .send_grease(true)
        .build(h3_quinn::Connection::new(c_conn))
        .await?;
    loop {
        tracing::debug!("Accepting an incoming request");
        match h3_conn.accept().await {
            Ok(Some(resolver)) => {
                tracing::debug!("Got the resolver");
                let (req, stream) = match resolver.resolve_request().await {
                    Ok(request) => request,
                    Err(err) => {
                        tracing::error!("Error resolving request: {err:?}");
                        continue;
                    }
                };
                tracing::debug!("New request: {:#?}", req);
                let ext = req.extensions();
                match req.method() {
                    &Method::CONNECT if ext.get::<Protocol>() == Some(&Protocol::WEB_TRANSPORT) => {
                        tracing::debug!("Peer wants to initiate a webtransport session");
                        tracing::debug!("Handing over connection to WebTransport");
                        let session = WebTransportSession::accept(req, stream, h3_conn)
                            .await?;
                        tracing::debug!("Established webtransport session");
                        // 4. Get datagrams, bidirectional streams, and unidirectional streams and wait for client requests here.
                        // h3_conn needs to hand over the datagrams, bidirectional streams, and unidirectional streams to the webtransport session.
                        // handle_session_and_echo_all_inbound_messages(session).await?;
                        let arc_session = Arc::new(session);
                        let web_transport_stream =
                            webtransport::accept_webtransport_stream(&arc_session).await?;

                        let certs = certhashes.iter().cloned().collect::<HashSet<_>>();
                        let t_noise = noise_config.with_webtransport_certhashes(certs);
                        let (peer_id, _) = t_noise.upgrade_inbound(web_transport_stream, "").await?;

                        let muxer = webtransport::Connection::new(arc_session, connection);

                        return Ok((peer_id, StreamMuxerBox::new(muxer)));
                    }
                    _ => {
                        tracing::info!(?req, "Received request");
                    }
                }
            }
            // indicating no more streams to be received
            Ok(None) => {
                tracing::error!("Indicating no more streams to be received");
                // indicating no more streams to be received
                return Err(WebtransportConnectingError::NoMoreStreams);
            }
            Err(err) => {
                tracing::error!("Connection errored with {}", err);

                return return Err(WebtransportConnectingError::Http3ConnectionError(err));
            }
        }
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, StreamMuxerBox), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (peer_id, connection) = match futures::ready!(self.connecting.poll_unpin(cx)) {
            Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
            Either::Left((res, _)) => res?,
        };

        Poll::Ready(Ok((peer_id, connection)))
    }
}
