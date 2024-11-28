use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::future::{BoxFuture, Either, Select, select};
use futures::FutureExt;
use futures_timer::Delay;
use wtransport::endpoint::IncomingSession;

use libp2p_core::StreamMuxer;
use libp2p_core::upgrade::InboundConnectionUpgrade;
use libp2p_identity::PeerId;

use crate::{Connection, Error};

pub(crate) const WEBTRANSPORT_PATH: &str = "/.well-known/libp2p-webtransport?type=noise";

/// A Webtransport connection currently being negotiated.
pub struct Connecting {
    noise_config: libp2p_noise::Config,
    // certhashes: Vec<CertHash>,
    connecting: Select<BoxFuture<'static, Result<(PeerId, Connection), Error>>, Delay>,
}

impl Connecting {
    pub fn new(incoming_session: IncomingSession,
               noise_config: libp2p_noise::Config,
               timeout: Duration,
               // certhashes: Vec<CertHash>,
    ) -> Self {
        Connecting {
            noise_config,
            // certhashes,
            connecting: select(
                Self::handshake(incoming_session).boxed(),
                Delay::new(timeout),
            ),
        }
    }

    async fn handshake(incoming_session: IncomingSession) -> Result<(PeerId, Connection), Error> {
        match incoming_session.await {
            Ok(session_request) => {
                let path = session_request.path();
                if path != WEBTRANSPORT_PATH {
                    return Err(
                        Error::UnexpectedPath(String::from(path))
                    );
                }

                match session_request.accept().await {
                    Ok(wtransport_connection) => {
                        let connection = Connection::new(wtransport_connection);
                        Ok((connection.remote_peer_id(), connection))
                    }
                    Err(connection_error) => {
                        Err(Error::Connection(connection_error))
                    }
                }
            }
            Err(connection_error) => {
                Err(Error::Connection(connection_error))
            }
        }
    }
}

impl Future for Connecting {
    type Output = Result<(PeerId, Connection), Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (peer_id, connection) = match futures::ready!(self.connecting.poll_unpin(cx)) {
            Either::Right(_) => return Poll::Ready(Err(Error::HandshakeTimedOut)),
            Either::Left((res, _)) => res?,
        };

        /*let session = WebTransportSession::accept(request, stream, h3_conn).await?;
                let arc_session = Arc::new(session);
                let webtr_stream =
                    webtransport::accept_webtransport_stream(&arc_session).await?;

                let certs = certhashes.iter().cloned().collect::<HashSet<_>>();
                let t_noise = noise_config.with_webtransport_certhashes(certs);
                t_noise.upgrade_inbound(webtr_stream, "").await?;

                let muxer = webtransport::Connection::new(arc_session, connection);

                return Ok((peer_id, StreamMuxerBox::new(muxer)));*/

        // futures::ready!(incoming.poll_unpin(cx))
        // t_noise.upgrade_inbound().await?;

        // todo здесь же нужно будет делать нойз хэндшейк

        Poll::Ready(Ok((peer_id, connection)))
    }
}