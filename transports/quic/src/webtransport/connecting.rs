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

//! Future that drives a QUIC connection until is has performed its TLS handshake.
//! And then it drives a http3 connection based on the QUIC connection.
//! And then it drives a WebTransport session based on the http3 connection.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{select, Select},
    prelude::*,
};
use futures::future::{BoxFuture, Either};
use futures_timer::Delay;
use h3::ext::Protocol;
use h3_webtransport::server::WebTransportSession;
use http::{Method, Request};

use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::InboundConnectionUpgrade;
use libp2p_identity::PeerId;

use crate::{Error, webtransport};
use crate::webtransport::connecting::WebTransportError::{Http3Error, NoiseError};

const WEBTRANSPORT_PATH: &str = "/.well-known/libp2p-webtransport";
const NOISE_QUERY: &str = "type=noise";

// #[derive(Debug)]
pub struct Connecting {
    connecting: Select<
        BoxFuture<'static, Result<(PeerId, StreamMuxerBox), WebTransportError>>,
        Delay
    >,
}

impl Connecting {
    pub(crate) fn new(
        noise: libp2p_noise::Config,
        connecting: quinn::Connecting, timeout: Duration) -> Self {
        Connecting {
            connecting: select(web_transport_connection(connecting, noise).boxed(), Delay::new(timeout))
        }
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, StreamMuxerBox), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = match futures::ready!(self.connecting.poll_unpin(cx)) {
            Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
            Either::Left((Ok((peer_id, muxer)), _)) => {
                Ok((peer_id, muxer))
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

async fn web_transport_connection(connecting: quinn::Connecting, _noise: libp2p_noise::Config)
                                  -> Result<(PeerId, StreamMuxerBox), WebTransportError> {
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

    match h3_conn.accept().await? {
        Some((request, stream)) => {
            let ext = request.extensions();
            let proto = ext.get::<Protocol>();
            if Some(&Protocol::WEB_TRANSPORT) == proto {
                if check_request(&request) {
                    let session = WebTransportSession::accept(request, stream, h3_conn)
                        .await?;
                    let connection = webtransport::Connection::new(session);

                    // let (_, out) = noise.upgrade_inbound(muxer, "").await?;

                    Ok((peer_id, StreamMuxerBox::new(connection)))
                } else {
                    Err(WebTransportError::BadRequest(request.method().clone()))
                }
            } else {
                Err(WebTransportError::UnexpectedProtocol(proto.cloned()))
            }
        }
        None => {
            // indicating no more streams to be received
            Err(WebTransportError::ClosedConnection)
        }
    }
}

fn check_request(req: &Request<()>) -> bool {
    req.method() == &Method::CONNECT &&
        req.uri().path() == WEBTRANSPORT_PATH &&
        req.uri().query() == Some(NOISE_QUERY)
}

// #[derive(Debug)]
pub enum WebTransportError {
    UnexpectedProtocol(Option<Protocol>),
    BadRequest(Method),
    ConnectionError(quinn::ConnectionError),
    ClosedConnection,
    Http3Error(h3::Error),
    NoiseError(libp2p_noise::Error),
}

impl From<libp2p_noise::Error> for WebTransportError {
    fn from(e: libp2p_noise::Error) -> Self {
        NoiseError(e)
    }
}

impl From<h3::Error> for WebTransportError {
    fn from(e: h3::Error) -> Self {
        Http3Error(e)
    }
}

impl From<quinn::ConnectionError> for WebTransportError {
    fn from(e: quinn::ConnectionError) -> Self {
        WebTransportError::ConnectionError(e)
    }
}