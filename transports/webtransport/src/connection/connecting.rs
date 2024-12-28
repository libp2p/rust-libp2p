use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::future::{select, BoxFuture, Either, Select};
use futures::{ready, FutureExt};
use futures_timer::Delay;
use wtransport::endpoint::IncomingSession;

use libp2p_core::upgrade::InboundConnectionUpgrade;
use libp2p_identity::PeerId;

use crate::{Connection, Error};

pub(crate) const WEBTRANSPORT_PATH: &str = "/.well-known/libp2p-webtransport?type=noise";

/// A Webtransport connection currently being negotiated.
pub struct Connecting {
    connecting: Select<BoxFuture<'static, Result<(PeerId, Connection), Error>>, Delay>,
}

impl Connecting {
    pub fn new(
        incoming_session: IncomingSession,
        noise_config: libp2p_noise::Config,
        timeout: Duration,
    ) -> Self {
        Connecting {
            connecting: select(
                Self::handshake(incoming_session, noise_config).boxed(),
                Delay::new(timeout),
            ),
        }
    }

    async fn handshake(
        incoming_session: IncomingSession,
        _noise_config: libp2p_noise::Config,
    ) -> Result<(PeerId, Connection), Error> {
        match incoming_session.await {
            Ok(session_request) => {
                tracing::debug!("Got session request={:?}", session_request.path());

                let path = session_request.path();
                if path != WEBTRANSPORT_PATH {
                    return Err(Error::UnexpectedPath(String::from(path)));
                }
                match session_request.accept().await {
                    Ok(wtransport_connection) => {
                        // The client SHOULD start the handshake right after sending the CONNECT request,
                        // without waiting for the server's response.
                        let peer_id = PeerId::random();
                        // todo a real noise auth
                        // let peer_id =
                        //     Self::noise_auth(wtransport_connection.clone(), noise_config).await?;

                        tracing::debug!(
                            "Accepted connection with sessionId={}",
                            wtransport_connection.session_id()
                        );

                        let connection = Connection::new(wtransport_connection);
                        Ok((peer_id, connection))
                    }
                    Err(connection_error) => Err(Error::Connection(connection_error)),
                }
            }
            Err(connection_error) => Err(Error::Connection(connection_error)),
        }
    }

    async fn noise_auth(
        connection: wtransport::Connection,
        noise_config: libp2p_noise::Config,
    ) -> Result<PeerId, Error> {
        fn remote_peer_id(con: &wtransport::Connection) -> PeerId {
            let cert_chain = con
                .peer_identity()
                .expect("connection got identity because it passed TLS handshake; qed");
            let cert = cert_chain
                .as_slice()
                .first()
                .expect("there should be exactly one certificate; qed");

            let p2p_cert = libp2p_tls::certificate::parse_binary(cert.der())
                .expect("the certificate was validated during TLS handshake; qed");

            p2p_cert.peer_id()
        }

        let (send, recv) = connection.accept_bi().await?;
        let stream = crate::Stream::new(send, recv);
        let (actual_peer_id, _) = noise_config.upgrade_inbound(stream, "").await?;

        let expected_peer_id = remote_peer_id(&connection);
        // TODO: This should be part libp2p-noise
        if actual_peer_id != expected_peer_id {
            return Err(Error::UnknownRemotePeerId);
        }

        Ok(actual_peer_id)
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, Connection), Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (peer_id, connection) = match ready!(self.connecting.poll_unpin(cx)) {
            Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
            Either::Left((res, _)) => res?,
        };

        Poll::Ready(Ok((peer_id, connection)))
    }
}
