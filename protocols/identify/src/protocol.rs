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

use crate::proto;
use asynchronous_codec::{FramedRead, FramedWrite};
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{
    multiaddr,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
    Multiaddr,
};
use libp2p_identity as identity;
use libp2p_identity::PublicKey;
use libp2p_swarm::StreamProtocol;
use log::{debug, trace};
use std::convert::TryFrom;
use std::{io, iter, pin::Pin};
use thiserror::Error;
use void::Void;

const MAX_MESSAGE_SIZE_BYTES: usize = 4096;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/id/1.0.0");

pub const PUSH_PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/id/push/1.0.0");

/// Substream upgrade protocol for `/ipfs/id/1.0.0`.
#[derive(Debug, Clone)]
pub struct Identify;

/// Substream upgrade protocol for `/ipfs/id/push/1.0.0`.
#[derive(Debug, Clone)]
pub struct Push<T>(T);
pub struct InboundPush();
pub struct OutboundPush(PartialInfo);

impl Push<InboundPush> {
    pub fn inbound() -> Self {
        Push(InboundPush())
    }
}

impl Push<OutboundPush> {
    pub fn outbound(info: PartialInfo) -> Self {
        Push(OutboundPush(info))
    }
}

/// Identify information of a peer sent in protocol messages.
#[derive(Debug, Clone)]
pub struct Info {
    /// The public key of the local peer.
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

impl From<Info> for PartialInfo {
    fn from(val: Info) -> Self {
        PartialInfo {
            public_key: Some(val.public_key),
            protocol_version: Some(val.protocol_version),
            agent_version: Some(val.agent_version),
            listen_addrs: val.listen_addrs,
            protocols: val.protocols,
            observed_addr: Some(val.observed_addr),
        }
    }
}

/// Partial identify information of a peer sent in protocol messages.
/// Note that missing fields should be ignored, as peers may choose to send partial updates containing only the fields whose values have changed.
#[derive(Debug, Clone)]
pub struct PartialInfo {
    pub public_key: Option<PublicKey>,
    pub protocol_version: Option<String>,
    pub agent_version: Option<String>,
    pub listen_addrs: Vec<Multiaddr>,
    pub protocols: Vec<StreamProtocol>,
    pub observed_addr: Option<Multiaddr>,
}

#[derive(Debug)]
pub enum IdentifyError {
    MissingRemotePublicKey,
}

impl TryFrom<PartialInfo> for Info {
    type Error = IdentifyError;

    fn try_from(info: PartialInfo) -> Result<Self, Self::Error> {
        Ok(Info {
            public_key: info
                .public_key
                .ok_or(IdentifyError::MissingRemotePublicKey)?,
            protocol_version: info.protocol_version.unwrap_or_default(),
            agent_version: info.agent_version.unwrap_or_default(),
            listen_addrs: info.listen_addrs,
            protocols: info.protocols,
            observed_addr: info.observed_addr.unwrap_or_else(Multiaddr::empty),
        })
    }
}

impl UpgradeInfo for Identify {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<C> InboundUpgrade<C> for Identify {
    type Output = C;
    type Error = UpgradeError;
    type Future = future::Ready<Result<Self::Output, UpgradeError>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        future::ok(socket)
    }
}

impl<C> OutboundUpgrade<C> for Identify
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = PartialInfo;
    type Error = UpgradeError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        recv(socket).boxed()
    }
}

impl<T> UpgradeInfo for Push<T> {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PUSH_PROTOCOL_NAME)
    }
}

impl<C> InboundUpgrade<C> for Push<InboundPush>
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = BoxFuture<'static, Result<PartialInfo, UpgradeError>>; // @todo -- Different future result?
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        // Lazily upgrade stream, thus allowing upgrade to happen within identify's handler.
        future::ok(recv(socket).boxed())
    }
}

impl<C> OutboundUpgrade<C> for Push<OutboundPush>
where
    C: AsyncWrite + Unpin + Send + 'static,
{
    type Output = ();
    type Error = UpgradeError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        send(socket, self.0 .0).boxed()
    }
}

pub(crate) async fn send<T>(io: T, info: PartialInfo) -> Result<(), UpgradeError>
where
    T: AsyncWrite + Unpin,
{
    trace!("Sending: {:?}", info);

    let listen_addrs = info
        .listen_addrs
        .into_iter()
        .map(|addr| addr.to_vec())
        .collect();

    let pubkey_bytes = info.public_key.map(|key| key.encode_protobuf());

    let message = proto::Identify {
        agentVersion: info.agent_version,
        protocolVersion: info.protocol_version,
        publicKey: pubkey_bytes,
        listenAddrs: listen_addrs,
        observedAddr: info.observed_addr.map(|addr| addr.to_vec()),
        protocols: info.protocols.into_iter().map(|p| p.to_string()).collect(),
    };

    let mut framed_io = FramedWrite::new(
        io,
        quick_protobuf_codec::Codec::<proto::Identify>::new(MAX_MESSAGE_SIZE_BYTES),
    );

    framed_io.send(message).await?;
    framed_io.close().await?;

    Ok(())
}

async fn recv<T>(socket: T) -> Result<PartialInfo, UpgradeError>
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
    .ok_or(UpgradeError::StreamClosed)??
    .try_into()?;

    trace!("Received: {:?}", info);

    Ok(info)
}

impl TryFrom<proto::Identify> for PartialInfo {
    type Error = UpgradeError;

    fn try_from(msg: proto::Identify) -> Result<Self, Self::Error> {
        fn parse_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, multiaddr::Error> {
            Multiaddr::try_from(bytes)
        }

        let listen_addrs = {
            let mut addrs = Vec::new();
            for addr in msg.listenAddrs.into_iter() {
                match parse_multiaddr(addr) {
                    Ok(a) => addrs.push(a),
                    Err(e) => {
                        debug!("Unable to parse listen multiaddr: {e:?}");
                    }
                }
            }
            addrs
        };

        let public_key = msg
            .publicKey
            .and_then(|key| match PublicKey::try_decode_protobuf(&key) {
                Ok(k) => Some(k),
                Err(e) => {
                    debug!("Unable to decode public key: {e:?}");
                    None
                }
            });

        let observed_addr = msg
            .observedAddr
            .and_then(|bytes| match parse_multiaddr(bytes) {
                Ok(a) => Some(a),
                Err(e) => {
                    debug!("Unable to parse observed multiaddr: {e:?}");
                    None
                }
            });

        let info = PartialInfo {
            public_key,
            protocol_version: msg.protocolVersion,
            agent_version: msg.agentVersion,
            listen_addrs,
            protocols: msg
                .protocols
                .into_iter()
                .filter_map(|p| match StreamProtocol::try_from_owned(p) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        debug!("Received invalid protocol from peer: {e}");
                        None
                    }
                })
                .collect(),
            observed_addr,
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
    use super::*;
    use libp2p_identity as identity;

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

        let info = PartialInfo::try_from(payload).expect("not to fail");

        assert_eq!(info.listen_addrs, vec![valid_multiaddr])
    }
}
