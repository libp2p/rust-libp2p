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
use libp2p_core::{multiaddr, Multiaddr, PeerRecord, SignedEnvelope};
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
    /// Verifiable addresses of the peer.
    pub signed_peer_record: Option<SignedEnvelope>,
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
        signedPeerRecord: info
            .signed_peer_record
            .clone()
            .map(|r| r.into_protobuf_encoding()),
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
        let identify_public_key = {
            match parse_public_key(msg.publicKey) {
                Some(key) => key,
                // This will always produce a DecodingError if the public key is missing.
                None => PublicKey::try_decode_protobuf(Default::default())?,
            }
        };

        // When signedPeerRecord contains valid addresses, ignore addresses in listenAddrs.
        // When signedPeerRecord is invalid or signed by others, ignore the signedPeerRecord(set to
        // `None`).
        let (listen_addrs, signed_envelope) = msg
            .signedPeerRecord
            .and_then(|b| {
                let envelope = SignedEnvelope::from_protobuf_encoding(b.as_ref()).ok()?;
                let peer_record = PeerRecord::from_signed_envelope(envelope).ok()?;
                (peer_record.peer_id() == identify_public_key.to_peer_id()).then_some((
                    peer_record.addresses().to_vec(),
                    Some(peer_record.into_signed_envelope()),
                ))
            })
            .unwrap_or_else(|| (parse_listen_addrs(msg.listenAddrs), None));

        let info = Info {
            public_key: identify_public_key,
            protocol_version: msg.protocolVersion.unwrap_or_default(),
            agent_version: msg.agentVersion.unwrap_or_default(),
            listen_addrs,
            protocols: parse_protocols(msg.protocols),
            observed_addr: parse_observed_addr(msg.observedAddr).unwrap_or(Multiaddr::empty()),
            signed_peer_record: signed_envelope,
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
    use std::str::FromStr;

    use libp2p_core::PeerRecord;
    use libp2p_identity as identity;
    use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};

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
            signedPeerRecord: None,
        };

        let info = PushInfo::try_from(payload).expect("not to fail");

        assert_eq!(info.listen_addrs, vec![valid_multiaddr])
    }

    #[test]
    fn protobuf_roundtrip() {
        // from go implementation of identify,
        // see https://github.com/libp2p/go-libp2p/blob/2209ae05976df6a1cc2631c961f57549d109008c/p2p/protocol/identify/pb/identify.pb.go#L133
        // signedPeerRecord field is a dummy one that can't be properly parsed into SignedEnvelope,
        // but the wire format doesn't care.
        let go_protobuf: [u8; 375] = [
            0x0a, 0x27, 0x70, 0x32, 0x70, 0x2f, 0x70, 0x72, 0x6f, 0x74, 0x6f, 0x63, 0x6f, 0x6c,
            0x2f, 0x69, 0x64, 0x65, 0x6e, 0x74, 0x69, 0x66, 0x79, 0x2f, 0x70, 0x62, 0x2f, 0x69,
            0x64, 0x65, 0x6e, 0x74, 0x69, 0x66, 0x79, 0x2e, 0x70, 0x72, 0x6f, 0x74, 0x6f, 0x12,
            0x0b, 0x69, 0x64, 0x65, 0x6e, 0x74, 0x69, 0x66, 0x79, 0x2e, 0x70, 0x62, 0x22, 0x86,
            0x02, 0x0a, 0x08, 0x49, 0x64, 0x65, 0x6e, 0x74, 0x69, 0x66, 0x79, 0x12, 0x28, 0x0a,
            0x0f, 0x70, 0x72, 0x6f, 0x74, 0x6f, 0x63, 0x6f, 0x6c, 0x56, 0x65, 0x72, 0x73, 0x69,
            0x6f, 0x6e, 0x18, 0x05, 0x20, 0x01, 0x28, 0x09, 0x52, 0x0f, 0x70, 0x72, 0x6f, 0x74,
            0x6f, 0x63, 0x6f, 0x6c, 0x56, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x12, 0x22, 0x0a,
            0x0c, 0x61, 0x67, 0x65, 0x6e, 0x74, 0x56, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x18,
            0x06, 0x20, 0x01, 0x28, 0x09, 0x52, 0x0c, 0x61, 0x67, 0x65, 0x6e, 0x74, 0x56, 0x65,
            0x72, 0x73, 0x69, 0x6f, 0x6e, 0x12, 0x1c, 0x0a, 0x09, 0x70, 0x75, 0x62, 0x6c, 0x69,
            0x63, 0x4b, 0x65, 0x79, 0x18, 0x01, 0x20, 0x01, 0x28, 0x0c, 0x52, 0x09, 0x70, 0x75,
            0x62, 0x6c, 0x69, 0x63, 0x4b, 0x65, 0x79, 0x12, 0x20, 0x0a, 0x0b, 0x6c, 0x69, 0x73,
            0x74, 0x65, 0x6e, 0x41, 0x64, 0x64, 0x72, 0x73, 0x18, 0x02, 0x20, 0x03, 0x28, 0x0c,
            0x52, 0x0b, 0x6c, 0x69, 0x73, 0x74, 0x65, 0x6e, 0x41, 0x64, 0x64, 0x72, 0x73, 0x12,
            0x22, 0x0a, 0x0c, 0x6f, 0x62, 0x73, 0x65, 0x72, 0x76, 0x65, 0x64, 0x41, 0x64, 0x64,
            0x72, 0x18, 0x04, 0x20, 0x01, 0x28, 0x0c, 0x52, 0x0c, 0x6f, 0x62, 0x73, 0x65, 0x72,
            0x76, 0x65, 0x64, 0x41, 0x64, 0x64, 0x72, 0x12, 0x1c, 0x0a, 0x09, 0x70, 0x72, 0x6f,
            0x74, 0x6f, 0x63, 0x6f, 0x6c, 0x73, 0x18, 0x03, 0x20, 0x03, 0x28, 0x09, 0x52, 0x09,
            0x70, 0x72, 0x6f, 0x74, 0x6f, 0x63, 0x6f, 0x6c, 0x73, 0x12, 0x2a, 0x0a, 0x10, 0x73,
            0x69, 0x67, 0x6e, 0x65, 0x64, 0x50, 0x65, 0x65, 0x72, 0x52, 0x65, 0x63, 0x6f, 0x72,
            0x64, 0x18, 0x08, 0x20, 0x01, 0x28, 0x0c, 0x52, 0x10, 0x73, 0x69, 0x67, 0x6e, 0x65,
            0x64, 0x50, 0x65, 0x65, 0x72, 0x52, 0x65, 0x63, 0x6f, 0x72, 0x64, 0x42, 0x36, 0x5a,
            0x34, 0x67, 0x69, 0x74, 0x68, 0x75, 0x62, 0x2e, 0x63, 0x6f, 0x6d, 0x2f, 0x6c, 0x69,
            0x62, 0x70, 0x32, 0x70, 0x2f, 0x67, 0x6f, 0x2d, 0x6c, 0x69, 0x62, 0x70, 0x32, 0x70,
            0x2f, 0x70, 0x32, 0x70, 0x2f, 0x70, 0x72, 0x6f, 0x74, 0x6f, 0x63, 0x6f, 0x6c, 0x2f,
            0x69, 0x64, 0x65, 0x6e, 0x74, 0x69, 0x66, 0x79, 0x2f, 0x70, 0x62,
        ];
        let mut buf = [0u8; 375];
        let mut message =
            proto::Identify::from_reader(&mut BytesReader::from_bytes(&go_protobuf), &go_protobuf)
                .expect("read to succeed");

        // The actual bytes they put in is "github.com/libp2p/go-libp2p/p2p/protocol/identify/pb".
        // Starting with Z4 means it is zig-zag-encoded 4-byte varint of string, appended by
        // protobuf.
        assert_eq!(
            String::from_utf8(
                message
                    .signedPeerRecord
                    .clone()
                    .expect("field to be present")
            )
            .expect("parse to succeed"),
            "Z4github.com/libp2p/go-libp2p/p2p/protocol/identify/pb".to_string()
        );
        message
            .write_message(&mut Writer::new(&mut buf[..]))
            .expect("same length after roundtrip");
        assert_eq!(go_protobuf, buf);

        let identity = identity::Keypair::generate_ed25519();
        let record = PeerRecord::new(
            &identity,
            vec![Multiaddr::from_str("/ip4/0.0.0.0").expect("parse to succeed")],
        )
        .expect("infallible siging using ed25519");
        message
            .signedPeerRecord
            .replace(record.into_signed_envelope().into_protobuf_encoding());
        let mut buf = Vec::new();
        message
            .write_message(&mut Writer::new(&mut buf))
            .expect("write to succeed");
        let parsed_message = proto::Identify::from_reader(&mut BytesReader::from_bytes(&buf), &buf)
            .expect("read to succeed");
        assert_eq!(message, parsed_message)
    }
}
