use crate::identity::Keypair;
use crate::signed_envelope::SignedEnvelope;
use crate::{peer_record_proto, Multiaddr, PeerId};
use std::convert::TryInto;

const PAYLOAD_TYPE: &str = "/libp2p/routing-state-record";
const DOMAIN_SEP: &str = "libp2p-routing-state";

// TODO: docs
#[derive(Debug, PartialEq, Clone)]
pub struct PeerRecord {
    pub peer_id: PeerId,
    pub seq: u64,
    pub addresses: Vec<Multiaddr>,
}

impl PeerRecord {
    // TODO: docs
    pub fn into_protobuf_encoding(self) -> Vec<u8> {
        use prost::Message;

        let record = peer_record_proto::PeerRecord {
            peer_id: self.peer_id.to_bytes(),
            seq: self.seq,
            addresses: self
                .addresses
                .into_iter()
                .map(|m| peer_record_proto::peer_record::AddressInfo {
                    multiaddr: m.to_vec(),
                })
                .collect(),
        };

        let mut buf = Vec::with_capacity(record.encoded_len());
        record
            .encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");
        buf
    }

    // TODO: wrap error to not expose `prost` as a dependency
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<Self, prost::DecodeError> {
        use prost::Message;

        let record = peer_record_proto::PeerRecord::decode(bytes)?;

        Ok(Self {
            peer_id: PeerId::from_bytes(&record.peer_id).unwrap(), // TODO: Error handling
            seq: record.seq,
            addresses: record
                .addresses
                .into_iter()
                .map(|a| a.multiaddr.try_into())
                .collect::<Result<Vec<_>, _>>()
                .unwrap(), // TODO: error handlign
        })
    }

    // TODO: docs
    pub fn warp_in_envelope(self, key: Keypair) -> SignedEnvelope {
        if key.public().into_peer_id() != self.peer_id {
            panic!("bad key")
        }

        let payload = self.into_protobuf_encoding();

        SignedEnvelope::new(
            key,
            String::from(DOMAIN_SEP),
            PAYLOAD_TYPE.as_bytes().to_vec(),
            payload,
        )
        .unwrap() // TODO: Error handling
    }

    pub fn authenticate(self, key: Keypair) -> AuthenticatedPeerRecord {
        AuthenticatedPeerRecord::from_record(key, self)
    }
}

// TODO: docs
#[derive(Debug)]
pub struct AuthenticatedPeerRecord {
    inner: PeerRecord,

    /// A signed envelope containing the above inner [`PeerRecord`].
    ///
    /// If this [`AuthenticatedPeerRecord`] was constructed from a [`SignedEnvelope`], this is the original instance.
    /// If this [`AuthenticatedPeerRecord`] was created by [`authenticating`](PeerRecord::authenticate) an existing [`PeerRecord`], then this is a pre-computed [`SignedEnvelope`] to make it easier to send an [`AuthenticatedPeerRecord`] across the wire.
    envelope: SignedEnvelope,
}

impl AuthenticatedPeerRecord {
    // TODO: docs
    pub fn from_signed_envelope(envelope: SignedEnvelope) -> Result<Self, ()> {
        let payload = envelope
            .payload(String::from(DOMAIN_SEP), PAYLOAD_TYPE.as_bytes())
            .unwrap(); // TODO: Error handling

        let record = PeerRecord::from_protobuf_encoding(payload).unwrap(); // TODO: Error handling

        Ok(Self {
            inner: record,
            envelope,
        })
    }

    pub fn from_record(key: Keypair, record: PeerRecord) -> Self {
        let envelope = record.clone().warp_in_envelope(key);

        Self {
            inner: record,
            envelope,
        }
    }

    pub fn to_signed_envelope(&self) -> SignedEnvelope {
        self.envelope.clone()
    }

    pub fn into_signed_envelope(self) -> SignedEnvelope {
        self.envelope
    }

    pub fn peer_id(&self) -> PeerId {
        self.inner.peer_id
    }

    pub fn seq(&self) -> u64 {
        self.inner.seq
    }

    pub fn addresses(&self) -> &[Multiaddr] {
        self.inner.addresses.as_slice()
    }
}

impl PartialEq<PeerRecord> for AuthenticatedPeerRecord {
    fn eq(&self, other: &PeerRecord) -> bool {
        self.inner.eq(other)
    }
}

impl PartialEq<AuthenticatedPeerRecord> for PeerRecord {
    fn eq(&self, other: &AuthenticatedPeerRecord) -> bool {
        other.inner.eq(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/ip4/127.0.0.1/tcp/1337";

    #[test]
    fn roundtrip_envelope() {
        let identity = Keypair::generate_ed25519();
        let record = PeerRecord {
            peer_id: identity.public().into_peer_id(),
            seq: 0,
            addresses: vec![HOME.parse().unwrap()],
        };

        let envelope = record.clone().warp_in_envelope(identity);
        let authenticated = AuthenticatedPeerRecord::from_signed_envelope(envelope).unwrap();

        assert_eq!(authenticated, record)
    }
}
