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

use crate::{connection::Inner, Connection, Error};

use futures::prelude::*;
use futures_timer::Delay;
use libp2p_core::PeerId;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

/// A QUIC connection currently being negotiated.
#[derive(Debug)]
pub struct Connecting {
    connection: Option<Inner>,
    timeout: Delay,
}

impl Connecting {
    /// Builds an [`Connecting`] that wraps around an [`Inner`] connection.
    pub(crate) fn new(inner: Inner, timeout: Duration) -> Self {
        Connecting {
            connection: Some(inner),
            timeout: Delay::new(timeout),
        }
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, Connection), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let connection = self
            .connection
            .as_mut()
            .expect("Future polled after it has completed");

        loop {
            let event = match connection.poll_event(cx) {
                Poll::Ready(Some(event)) => event,
                Poll::Ready(None) => return Poll::Ready(Err(Error::EndpointDriverCrashed)),
                Poll::Pending => {
                    return self
                        .timeout
                        .poll_unpin(cx)
                        .map(|()| Err(Error::HandshakeTimedOut));
                }
            };
            match event {
                quinn_proto::Event::Connected => {
                    let session = connection.crypto_session();
                    let identity = session
                        .peer_identity()
                        .expect("connection got identity because it passed TLS handshake; qed");
                    let certificates: Box<Vec<rustls::Certificate>> =
                        identity.downcast().expect("we rely on rustls feature; qed");
                    let end_entity = certificates
                        .get(0)
                        .expect("there should be exactly one certificate; qed");
                    let end_entity_der = end_entity.as_ref();
                    let p2p_cert = crate::tls::certificate::parse_certificate(end_entity_der)
                        .expect("the certificate was validated during TLS handshake; qed");
                    let peer_id = PeerId::from_public_key(&p2p_cert.extension.public_key);

                    let connection = Connection::new(self.connection.take().unwrap());
                    return Poll::Ready(Ok((peer_id, connection)));
                }
                quinn_proto::Event::ConnectionLost { reason } => {
                    return Poll::Ready(Err(Error::Connection(reason.into())))
                }
                quinn_proto::Event::HandshakeDataReady | quinn_proto::Event::Stream(_) => {}
                quinn_proto::Event::DatagramReceived => {
                    debug_assert!(false, "Datagrams are not supported")
                }
            }
        }
    }
}
