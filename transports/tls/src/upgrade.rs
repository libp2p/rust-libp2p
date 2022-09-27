use crate::certificate;
use crate::certificate::P2pCertificate;
use futures::future::BoxFuture;
use futures::AsyncWrite;
use futures::{AsyncRead, FutureExt};
use futures_rustls::TlsStream;
use libp2p_core::{identity, InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use rustls::{CommonState, ServerName};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to generate certificate")]
    CertificateGeneration(#[from] certificate::GenError),
    #[error("Failed to upgrade server connection")]
    ServerUpgrade(std::io::Error),
    #[error("Failed to upgrade client connection")]
    ClientUpgrade(std::io::Error),
    #[error("Failed to parse certificate")]
    BadCertificate(#[from] certificate::ParseError),
}

#[derive(Clone)]
pub struct Config {
    server: rustls::ServerConfig,
    client: rustls::ClientConfig,
}

impl Config {
    pub fn new(identity: &identity::Keypair) -> Result<Self, certificate::GenError> {
        Ok(Self {
            server: crate::make_server_config(identity)?,
            client: crate::make_client_config(identity)?,
        })
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(b"/tls/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = (PeerId, TlsStream<C>);
    type Error = Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        async move {
            let stream = futures_rustls::TlsAcceptor::from(Arc::new(self.server))
                .accept(socket)
                .await
                .map_err(Error::ServerUpgrade)?;

            let peer_id = extract_single_certificate(stream.get_ref().1)?.peer_id();

            Ok((peer_id, stream.into()))
        }
        .boxed()
    }
}

impl<C> OutboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = (PeerId, TlsStream<C>);
    type Error = Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        async move {
            // Spec: In order to keep this flexibility for future versions, clients that only support the version of the handshake defined in this document MUST NOT send any value in the Server Name Indication.
            // Setting `ServerName` to unspecified will disable the use of the SNI extension.
            let name = ServerName::IpAddress(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

            let stream = futures_rustls::TlsConnector::from(Arc::new(self.client))
                .connect(name, socket)
                .await
                .map_err(Error::ClientUpgrade)?;

            let peer_id = extract_single_certificate(stream.get_ref().1)?.peer_id();

            Ok((peer_id, stream.into()))
        }
        .boxed()
    }
}

fn extract_single_certificate(
    state: &CommonState,
) -> Result<P2pCertificate<'_>, certificate::ParseError> {
    let cert = match state
        .peer_certificates()
        .expect("config enforces presence of certificates")
    {
        [single] => single,
        _ => panic!("config enforces exactly one certificate"),
    };

    certificate::parse(cert)
}
