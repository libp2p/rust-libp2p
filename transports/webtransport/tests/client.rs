use libp2p_core::upgrade::OutboundConnectionUpgrade;
use libp2p_core::UpgradeInfo;
use libp2p_identity::{Keypair, PeerId};
use libp2p_webtransport as webtransport;
use libp2p_webtransport::{CertHash, Stream};
use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use wtransport::error::{ConnectingError, ConnectionError, StreamOpeningError};
use wtransport::{ClientConfig, Connection, Endpoint};

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClientError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Connecting(#[from] ConnectingError),

    #[error(transparent)]
    Connection(#[from] ConnectionError),

    #[error(transparent)]
    Stream(#[from] StreamOpeningError),
}

pub(crate) struct WtClient {
    url: String,
    remote_peer_id: PeerId,
    keypair: Keypair,
    certhashes: HashSet<CertHash>,
    connection: Option<Arc<Connection>>,
}

impl WtClient {
    pub(crate) fn new(url: String, remote_peer_id: PeerId, certhashes: HashSet<CertHash>) -> Self {
        let keypair = Keypair::generate_ed25519();
        WtClient {
            url,
            remote_peer_id,
            keypair,
            certhashes,
            connection: None,
        }
    }

    pub(crate) async fn connect(&mut self) -> Result<Stream, ClientError> {
        let client_tls = libp2p_tls::make_webtransport_client_config(
            Some(self.remote_peer_id),
            alpn_protocols(),
        );

        let config = ClientConfig::builder()
            .with_bind_default()
            .with_custom_tls(client_tls)
            .build();

        tracing::debug!("Connecting to {}", self.url.as_str());

        let con = Endpoint::client(config)?.connect(self.url.as_str()).await?;
        self.connection = Some(Arc::new(con));

        tracing::debug!("Connection is established to {}", self.url.as_str());

        let _ = self.authenticate().await?;
        let stream = self.create_bidirectional_stream().await?;

        Ok(stream)
    }

    async fn create_bidirectional_stream(&mut self) -> Result<Stream, ClientError> {
        match &self.connection {
            None => {
                panic!("There is no open connection!")
            }
            Some(arc_con) => {
                let con = Arc::clone(arc_con);
                let opening_bi_stream = con.open_bi().await?;

                tracing::debug!("Opening bi stream is created");

                let (send, recv) = opening_bi_stream.await?;
                let stream = webtransport::Stream::new(send, recv);

                tracing::debug!("Stream is created");

                Ok(stream)
            }
        }
    }

    async fn authenticate(&mut self) -> Result<(), ClientError> {
        let stream = self.create_bidirectional_stream().await?;
        let mut noise = libp2p_noise::Config::new(&self.keypair).unwrap();

        if !self.certhashes.is_empty() {
            noise = noise.with_webtransport_certhashes(self.certhashes.clone());
        }

        tracing::debug!("Noise upgrade_outbound");
        let info = noise.protocol_info().next().unwrap_or_default();
        let _ = noise.upgrade_outbound(stream, info).await.unwrap();

        Ok(())
    }
}

fn alpn_protocols() -> Vec<Vec<u8>> {
    vec![b"libp2p".to_vec(), b"h3".to_vec()]
}
