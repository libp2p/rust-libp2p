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

use std::collections::HashSet;
use std::sync::Arc;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::future::BoxFuture;
use futures::{
    future::{select, Either, FutureExt, Select},
    prelude::*,
};
use futures_timer::Delay;
use h3::error::ErrorLevel;
use h3::ext::Protocol;
use h3_webtransport::server::WebTransportSession;
use http::Method;
use quinn::rustls::pki_types::CertificateDer;

use libp2p_core::multihash::Multihash;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::InboundConnectionUpgrade;
use libp2p_identity::PeerId;

use crate::transport::ConnectingMode;
use crate::webtransport::WebtransportConnectingError;
use crate::{webtransport, Connection, ConnectionError, Error};

/// A QUIC connection currently being negotiated.
pub struct Connecting {
    connecting: Select<BoxFuture<'static, Result<(PeerId, StreamMuxerBox), Error>>, Delay>,
}

impl Connecting {
    pub(crate) fn new(
        connection: quinn::Connecting,
        mode: ConnectingMode,
        timeout: Duration,
    ) -> Self {
        Connecting {
            connecting: select(handshake(connection, mode).boxed(), Delay::new(timeout)),
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
            let peer_id = Connecting::remote_peer_id(&connection);
            match mode {
                ConnectingMode::QUIC => {
                    let muxer = Connection::new(connection);
                    return Ok((peer_id, StreamMuxerBox::new(muxer)));
                }
                ConnectingMode::WebTransport(certhashes, noise_config) => {
                    let (peer_id, muxer) = webtransport_connecting(
                        peer_id,
                        connection.clone(),
                        certhashes,
                        noise_config,
                    )
                    .await?;
                    Ok((peer_id, StreamMuxerBox::new(muxer)))
                }
                ConnectingMode::Mixed(certhashes, noise_config) => {
                    let res = webtransport_connecting(
                        peer_id,
                        connection.clone(),
                        certhashes,
                        noise_config,
                    )
                    .await;
                    match res {
                        Ok((peer_id, muxer)) => Ok((peer_id, StreamMuxerBox::new(muxer))),
                        Err(WebtransportConnectingError::UnexpectedProtocol(_)) => {
                            let muxer = Connection::new(connection);
                            Ok((peer_id, StreamMuxerBox::new(muxer)))
                        }
                        Err(e) => Err(e.into()),
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
    peer_id: PeerId,
    connection: quinn::Connection,
    certhashes: Vec<Multihash<64>>,
    noise_config: libp2p_noise::Config,
) -> Result<(PeerId, StreamMuxerBox), WebtransportConnectingError> {
    loop {
        let mut h3_conn = h3::server::builder()
            .enable_webtransport(true)
            .enable_connect(true)
            .enable_datagram(false)
            .max_webtransport_sessions(1)
            .send_grease(true)
            .build(h3_quinn::Connection::new(connection.clone()))
            .await?;

        match h3_conn.accept().await {
            Ok(Some((request, stream))) => {
                let ext = request.extensions();
                let proto = ext.get::<Protocol>();
                if Some(&Protocol::WEB_TRANSPORT) == proto {
                    let method = request.method();
                    if method != &Method::CONNECT {
                        return Err(WebtransportConnectingError::UnexpectedMethod(
                            method.clone(),
                        ));
                    }
                    if request.uri().path() != webtransport::WEBTRANSPORT_PATH {
                        return Err(WebtransportConnectingError::UnexpectedPath(String::from(
                            request.uri().path(),
                        )));
                    }
                    if request.uri().query() == Some(webtransport::NOISE_QUERY) {
                        return Err(WebtransportConnectingError::UnexpectedQuery(String::from(
                            request.uri().path(),
                        )));
                    }
                    let session = WebTransportSession::accept(request, stream, h3_conn).await?;
                    let arc_session = Arc::new(session);
                    let webtr_stream =
                        webtransport::accept_webtransport_stream(&arc_session).await?;

                    let mut certs = HashSet::new();
                    certs.insert(certhashes.first().unwrap().clone());
                    let t_noise = noise_config.with_webtransport_certhashes(certs);
                    t_noise.upgrade_inbound(webtr_stream, "").await?;

                    let muxer = webtransport::Connection::new(arc_session, connection);

                    return Ok((peer_id, StreamMuxerBox::new(muxer)));
                } else {
                    return Err(WebtransportConnectingError::UnexpectedProtocol(
                        proto.cloned(),
                    ));
                }
            }
            Ok(None) => {
                // indicating no more streams to be received
                return Err(WebtransportConnectingError::NoMoreStreams);
            }
            Err(err) => match err.get_error_level() {
                ErrorLevel::ConnectionError => {
                    return Err(WebtransportConnectingError::Http3Error(err))
                }
                ErrorLevel::StreamError => continue,
            },
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
