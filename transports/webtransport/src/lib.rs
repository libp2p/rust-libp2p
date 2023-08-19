use futures::{future::BoxFuture, ready, FutureExt, SinkExt, StreamExt};
use h3::{error::ErrorLevel, ext::Protocol};
use libp2p_core::{muxing::StreamMuxerBox, transport::TransportEvent};
use libp2p_identity::PeerId;
use std::{net::SocketAddr, sync::Arc, task::Poll};

use rustls::{Certificate, PrivateKey};

mod fingerprint;

const P2P_ALPN: [u8; 6] = *b"libp2p";

pub struct Transport(
    futures::channel::mpsc::Receiver<h3::server::Connection<h3_quinn::Connection, bytes::Bytes>>,
);

impl Transport {
    pub fn new(peer_id: PeerId) -> Result<Self, Box<dyn std::error::Error>> {
        let mut params = rcgen::CertificateParams::new(vec![
            "hello.world.example".to_string(),
            "localhost".to_string(),
        ]);

        // Set not_before and not_after
        params.not_before = time::OffsetDateTime::now_utc();
        // TODO: Obviously we can do better.
        params.not_after =
            time::OffsetDateTime::now_utc() + std::time::Duration::from_secs(60 * 60 * 24);

        let x = rcgen::Certificate::from_params(params).unwrap();
        let cert = Certificate(x.serialize_der().unwrap());

        let fingerprint = fingerprint::Fingerprint::from_certificate(cert.as_ref());
        let key = PrivateKey(x.serialize_private_key_der());

        let socket_addr: SocketAddr = "[::1]:4433".parse().unwrap();

        let addr = libp2p::core::Multiaddr::empty()
            .with(socket_addr.ip().into())
            .with(libp2p::core::multiaddr::Protocol::Udp(socket_addr.port()))
            .with(libp2p::core::multiaddr::Protocol::QuicV1)
            .with(libp2p::core::multiaddr::Protocol::WebTransport)
            .with(libp2p::core::multiaddr::Protocol::Certhash(
                fingerprint.to_multihash(),
            ))
            .with(libp2p::core::multiaddr::Protocol::P2p(peer_id));

        println!("Listening on: {addr:?}");

        let mut tls_config = rustls::ServerConfig::builder()
            .with_safe_default_cipher_suites()
            .with_safe_default_kx_groups()
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?;

        tls_config.max_early_data_size = u32::MAX;
        let alpn: Vec<Vec<u8>> = vec![
            b"h3".to_vec(),
            b"h3-32".to_vec(),
            b"h3-31".to_vec(),
            b"h3-30".to_vec(),
            b"h3-29".to_vec(),
        ];
        tls_config.alpn_protocols = alpn;

        let server_config = quinn::ServerConfig::with_crypto(Arc::new(tls_config));
        let endpoint = quinn::Endpoint::server(server_config, socket_addr)?;

        let (sender, receiver) = futures::channel::mpsc::channel(0);

        tokio::spawn(async move {
            log::debug!("spawned");
            while let Some(new_conn) = endpoint.accept().await {
                log::debug!("New connection being attempted");

                let mut sender = sender.clone();

                tokio::spawn(async move {
                    match new_conn.await {
                        Ok(conn) => {
                            log::info!("new connection established");

                            let h3_conn = h3::server::builder()
                                .enable_webtransport(true)
                                .enable_connect(true)
                                .enable_datagram(true)
                                .max_webtransport_sessions(1)
                                .send_grease(true)
                                .build(h3_quinn::Connection::new(conn))
                                .await
                                .unwrap();

                            sender.send(h3_conn).await.unwrap();
                        }
                        Err(err) => {
                            log::error!("accepting connection failed: {:?}", err);
                        }
                    }
                });
            }
        });

        Ok(Transport(receiver))
    }
}

impl libp2p_core::Transport for Transport {
    type Output = (PeerId, StreamMuxerBox);

    type Error = std::io::Error;

    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    type Dial = futures::future::Pending<Result<Self::Output, std::io::Error>>;

    fn listen_on(
        &mut self,
        _id: libp2p_core::transport::ListenerId,
        _addr: libp2p_core::Multiaddr,
    ) -> Result<(), libp2p_core::transport::TransportError<Self::Error>> {
        Ok(())
    }

    fn remove_listener(&mut self, _id: libp2p_core::transport::ListenerId) -> bool {
        todo!()
    }

    fn dial(
        &mut self,
        _addr: libp2p_core::Multiaddr,
    ) -> Result<Self::Dial, libp2p_core::transport::TransportError<Self::Error>> {
        todo!()
    }

    fn dial_as_listener(
        &mut self,
        _addr: libp2p_core::Multiaddr,
    ) -> Result<Self::Dial, libp2p_core::transport::TransportError<Self::Error>> {
        todo!()
    }

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>>
    {
        let connection = ready!(self.0.poll_next_unpin(cx)).unwrap();

        handle(connection);

        Poll::Ready(TransportEvent::Incoming {
            listener_id: todo!(),
            upgrade: handle(connection).boxed(),
            local_addr: todo!(),
            send_back_addr: todo!(),
        })
    }

    fn address_translation(
        &self,
        _listen: &libp2p_core::Multiaddr,
        _observed: &libp2p_core::Multiaddr,
    ) -> Option<libp2p_core::Multiaddr> {
        todo!()
    }
}

async fn handle(
    mut conn: h3::server::Connection<h3_quinn::Connection, bytes::Bytes>,
) -> Result<(PeerId, StreamMuxerBox), std::io::Error> {
    loop {
        match conn.accept().await {
            Ok(Some((req, stream))) => {
                log::info!("new request: {:#?}", req);

                let ext = req.extensions();
                match req.method() {
                    &http::Method::CONNECT
                        if ext.get::<Protocol>() == Some(&Protocol::WEB_TRANSPORT) =>
                    {
                        log::info!("Peer wants to initiate a webtransport session");

                        log::info!("Handing over connection to WebTransport");
                        let _session =
                            h3_webtransport::server::WebTransportSession::accept(req, stream, conn)
                                .await
                                .unwrap();
                        log::info!("Established webtransport session");
                        // 4. Get datagrams, bidirectional streams, and unidirectional streams and wait for client requests here.
                        // h3_conn needs to handover the datagrams, bidirectional streams, and unidirectional streams to the webtransport session.
                        // handle_session_and_echo_all_inbound_messages(session).await?;

                        todo!()
                    }
                    _ => {
                        log::info!("Received request");
                    }
                }
            }

            // indicating no more streams to be received
            Ok(None) => {
                break;
            }

            Err(err) => {
                log::error!("Error on accept {}", err);
                match err.get_error_level() {
                    ErrorLevel::ConnectionError => break,
                    ErrorLevel::StreamError => continue,
                }
            }
        }
    }

    todo!()
}
