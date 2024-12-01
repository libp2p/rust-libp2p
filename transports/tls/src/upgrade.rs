// Copyright 2022 Protocol Labs.
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

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

use futures::{future::BoxFuture, AsyncRead, AsyncWrite, FutureExt};
use futures_rustls::TlsStream;
use libp2p_core::{
    upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade},
    UpgradeInfo,
};
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use rustls::{pki_types::ServerName, CommonState};

use crate::{certificate, certificate::P2pCertificate};

#[derive(thiserror::Error, Debug)]
pub enum UpgradeError {
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
            client: crate::make_client_config(identity, None)?,
        })
    }
}

impl UpgradeInfo for Config {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once("/tls/1.0.0")
    }
}

impl<C> InboundConnectionUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = (PeerId, TlsStream<C>);
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        async move {
            let stream = futures_rustls::TlsAcceptor::from(Arc::new(self.server))
                .accept(socket)
                .await
                .map_err(UpgradeError::ServerUpgrade)?;

            let peer_id = extract_single_certificate(stream.get_ref().1)?.peer_id();

            Ok((peer_id, stream.into()))
        }
        .boxed()
    }
}

impl<C> OutboundConnectionUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = (PeerId, TlsStream<C>);
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        async move {
            // Spec: In order to keep this flexibility for future versions, clients that only
            // support the version of the handshake defined in this document MUST NOT send any value
            // in the Server Name Indication. Setting `ServerName` to unspecified will
            // disable the use of the SNI extension.
            let name = ServerName::IpAddress(rustls::pki_types::IpAddr::from(IpAddr::V4(
                Ipv4Addr::UNSPECIFIED,
            )));

            let stream = futures_rustls::TlsConnector::from(Arc::new(self.client))
                .connect(name, socket)
                .await
                .map_err(UpgradeError::ClientUpgrade)?;

            let peer_id = extract_single_certificate(stream.get_ref().1)?.peer_id();

            Ok((peer_id, stream.into()))
        }
        .boxed()
    }
}

fn extract_single_certificate(
    state: &CommonState,
) -> Result<P2pCertificate<'_>, certificate::ParseError> {
    let Some([cert]) = state.peer_certificates() else {
        panic!("config enforces exactly one certificate");
    };

    certificate::parse(cert)
}
