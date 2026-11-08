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
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{Either, FutureExt, Select, select},
    prelude::*,
};
use futures_timer::Delay;
use libp2p_identity::PeerId;
use quinn::rustls::pki_types::CertificateDer;
use quinn_proto::TransportError;

use crate::{Connection, ConnectionError, Error};

/// A QUIC connection currently being negotiated.
#[derive(Debug)]
pub struct Connecting {
    connecting: Select<quinn::Connecting, Delay>,
}

impl Connecting {
    pub(crate) fn new(connection: quinn::Connecting, timeout: Duration) -> Self {
        Connecting {
            connecting: select(connection, Delay::new(timeout)),
        }
    }
}

impl Connecting {
    fn remote_peer_id(connection: &quinn::Connection) -> Result<PeerId, Error> {
        fn transport_err(reason: &str) -> Error {
            Error::Connection(ConnectionError(quinn::ConnectionError::TransportError(
                TransportError {
                    code: quinn::TransportErrorCode::PROTOCOL_VIOLATION,
                    frame: None,
                    reason: reason.to_string(),
                },
            )))
        }

        let identity = connection
            .peer_identity()
            .ok_or_else(|| transport_err("No crypto identity in quinn's Connection"))?;
        let certificates: Box<Vec<CertificateDer>> = identity
            .downcast()
            .map_err(|_| transport_err("Could not downcast identity"))?;
        let end_entity = certificates
            .first()
            .ok_or_else(|| transport_err("No certificate found"))?;
        let p2p_cert = libp2p_tls::certificate::parse(end_entity)
            .map_err(|_| transport_err("Could not parse certificate"))?;
        Ok(p2p_cert.peer_id())
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, Connection), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let connection = match futures::ready!(self.connecting.poll_unpin(cx)) {
            Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
            Either::Left((connection, _)) => connection.map_err(ConnectionError)?,
        };

        let peer_id = Self::remote_peer_id(&connection)?;
        let muxer = Connection::new(connection);
        Poll::Ready(Ok((peer_id, muxer)))
    }
}
