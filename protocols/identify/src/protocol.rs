// Copyright 2018 Parity Technologies (UK) Ltd.
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

use std::io;

use asynchronous_codec::{FramedRead, FramedWrite};
use futures::prelude::*;
use libp2p_core::{multiaddr, Multiaddr};
use libp2p_identity as identity;
use libp2p_identity::PublicKey;
use libp2p_swarm::StreamProtocol;
use thiserror::Error;

use crate::proto;

const MAX_MESSAGE_SIZE_BYTES: usize = 4096;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/id/1.0.0");

pub const PUSH_PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/id/push/1.0.0");

/// Identify information of a peer sent in protocol messages.
#[derive(Debug, Clone)]
pub struct Info {
    /// The public key of the peer.
    pub public_key: PublicKey,
    /// Application-specific version of the protocol family used by the peer,
    /// e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    pub agent_version: String,
    /// The addresses that the peer is listening on.
    pub listen_addrs: Vec<Multiaddr>,
    /// The list of protocols supported by the peer, e.g. `/ipfs/ping/1.0.0`.
    pub protocols: Vec<StreamProtocol>,
    /// Address observed by or for the remote.
    pub observed_addr: Multiaddr,
}

impl Info {
    pub fn merge(&mut self, info: PushInfo) {
        if let Some(public_key) = info.public_key {
            self.public_key = public_key;
        }
        if let Some(protocol_version) = info.protocol_version {
            self.protocol_version = protocol_version;
        }
        if let Some(agent_version) = info.agent_version {
            self.agent_version = agent_version;
        }
        if !info.listen_addrs.is_empty() {
            self.listen_addrs = info.listen_addrs;
        }
        if !info.protocols.is_empty() {
            self.protocols = info.protocols;
        }
        if let Some(observed_addr) = info.observed_addr {
            self.observed_addr = observed_addr;
        }
    }
}

/// Identify push information of a peer sent in protocol messages.
/// Note that missing fields should be ignored, as peers may choose to send partial updates
/// containing only the fields whose values have changed.
#[derive(Debug, Clone)]
pub struct PushInfo {
    pub public_key: Option<PublicKey>,
    pub protocol_version: Option<String>,
    pub agent_version: Option<String>,
    pub listen_addrs: Vec<Multiaddr>,
    pub protocols: Vec<StreamProtocol>,
    pub observed_addr: Option<Multiaddr>,
}

pub(crate) async fn send_identify<T>(io: T, info: Info) -> Result<Info, UpgradeError>
where
    T: AsyncWrite + Unpin,
{
    tracing::trace!("Sending: {:?}", info);

    let listen_addrs = info.listen_addrs.iter().map(|addr| addr.to_vec()).collect();

    let pubkey_bytes = info.public_key.encode_protobuf();

    let message = proto::Identify {
        agentVersion: Some(info.agent_version.clone()),
        protocolVersion: Some(info.protocol_version.clone()),
        publicKey: Some(pubkey_bytes),
        listenAddrs: listen_addrs,
        observedAddr: Some(info.observed_addr.to_vec()),
        protocols: info.protocols.iter().map(|p| p.to_string()).collect(),
    };

    let mut framed_io = FramedWrite::new(
        io,
        quick_protobuf_codec::Codec::<proto::Identify>::new(MAX_MESSAGE_SIZE_BYTES),
    );

    framed_io.send(message).await?;
    framed_io.close().await?;

    Ok(info)
}

pub(crate) async fn recv_push<T>(socket: T) -> Result<PushInfo, UpgradeError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let info = recv(socket).await?.try_into()?;

    tracing::trace!(?info, "Received");

    Ok(info)
}

pub(crate) async fn recv_identify<T>(socket: T) -> Result<Info, UpgradeError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let info = recv(socket).await?.try_into()?;

    tracing::trace!(?info, "Received");

    Ok(info)
}

async fn recv<T>(socket: T) -> Result<proto::Identify, UpgradeError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    // Even though we won't write to the stream anymore we don't close it here.
    // The reason for this is that the `close` call on some transport's require the
    // remote's ACK, but it could be that the remote already dropped the stream
    // after finishing their write.

    let info = FramedRead::new(
        socket,
        quick_protobuf_codec::Codec::<proto::Identify>::new(MAX_MESSAGE_SIZE_BYTES),
    )
    .next()
    .await
    .ok_or(UpgradeError::StreamClosed)??;

    Ok(info)
}

fn parse_listen_addrs(listen_addrs: Vec<Vec<u8>>) -> Vec<Multiaddr> {
    listen_addrs
        .into_iter()
        .filter_map(|bytes| match Multiaddr::try_from(bytes) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::debug!("Unable to parse multiaddr: {e:?}");
                None
            }
        })
        .collect()
}

fn parse_protocols(protocols: Vec<String>) -> Vec<StreamProtocol> {
    protocols
        .into_iter()
        .filter_map(|p| match StreamProtocol::try_from_owned(p) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::debug!("Received invalid protocol from peer: {e}");
                None
            }
        })
        .collect()
}

fn parse_public_key(public_key: Option<Vec<u8>>) -> Option<PublicKey> {
    public_key.and_then(|key| match PublicKey::try_decode_protobuf(&key) {
        Ok(k) => Some(k),
        Err(e) => {
            tracing::debug!("Unable to decode public key: {e:?}");
            None
        }
    })
}

fn parse_observed_addr(observed_addr: Option<Vec<u8>>) -> Option<Multiaddr> {
    observed_addr.and_then(|bytes| match Multiaddr::try_from(bytes) {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::debug!("Unable to parse observed multiaddr: {e:?}");
            None
        }
    })
}

impl TryFrom<proto::Identify> for Info {
    type Error = UpgradeError;

    fn try_from(msg: proto::Identify) -> Result<Self, Self::Error> {
        let public_key = {
            match parse_public_key(msg.publicKey) {
                Some(key) => key,
                // This will always produce a DecodingError if the public key is missing.
                None => PublicKey::try_decode_protobuf(Default::default())?,
            }
        };

        let info = Info {
            public_key,
            protocol_version: msg.protocolVersion.unwrap_or_default(),
            agent_version: msg.agentVersion.unwrap_or_default(),
            listen_addrs: parse_listen_addrs(msg.listenAddrs),
            protocols: parse_protocols(msg.protocols),
            observed_addr: parse_observed_addr(msg.observedAddr).unwrap_or(Multiaddr::empty()),
        };

        Ok(info)
    }
}

impl TryFrom<proto::Identify> for PushInfo {
    type Error = UpgradeError;

    fn try_from(msg: proto::Identify) -> Result<Self, Self::Error> {
        let info = PushInfo {
            public_key: parse_public_key(msg.publicKey),
            protocol_version: msg.protocolVersion,
            agent_version: msg.agentVersion,
            listen_addrs: parse_listen_addrs(msg.listenAddrs),
            protocols: parse_protocols(msg.protocols),
            observed_addr: parse_observed_addr(msg.observedAddr),
        };

        Ok(info)
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("I/O interaction failed")]
    Io(#[from] io::Error),
    #[error("Stream closed")]
    StreamClosed,
    #[error("Failed decoding multiaddr")]
    Multiaddr(#[from] multiaddr::Error),
    #[error("Failed decoding public key")]
    PublicKey(#[from] identity::DecodingError),
}

#[cfg(test)]
mod tests {
    use libp2p_identity as identity;

    use super::*;

    #[test]
    fn skip_invalid_multiaddr() {
        let valid_multiaddr: Multiaddr = "/ip6/2001:db8::/tcp/1234".parse().unwrap();
        let valid_multiaddr_bytes = valid_multiaddr.to_vec();

        let invalid_multiaddr = {
            let a = vec![255; 8];
            assert!(Multiaddr::try_from(a.clone()).is_err());
            a
        };

        let payload = proto::Identify {
            agentVersion: None,
            listenAddrs: vec![valid_multiaddr_bytes, invalid_multiaddr],
            observedAddr: None,
            protocolVersion: None,
            protocols: vec![],
            publicKey: Some(
                identity::Keypair::generate_ed25519()
                    .public()
                    .encode_protobuf(),
            ),
        };

        let info = PushInfo::try_from(payload).expect("not to fail");

        assert_eq!(info.listen_addrs, vec![valid_multiaddr])
    }
}
