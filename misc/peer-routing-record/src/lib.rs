use libp2p_core::identity::Keypair;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_signed_envelope::{Envelope, Error, Record};
use log::error;
use prost::Message;
use std::convert::TryFrom;
use std::time::{SystemTime, UNIX_EPOCH};

mod peer_record_proto {
    include!(concat!(env!("OUT_DIR"), "/peer_record_proto.rs"));
}

#[derive(Clone, Debug, PartialEq)]
struct PeerRecord {
    peer_id: PeerId,
    addresses: Vec<Multiaddr>,
    seq: u64,
}

#[derive(Debug)]
pub enum DecodeError {
    ProtoBufDecodingError(prost::DecodeError),
    PeerIdDecodingError(Vec<u8>),
    MultiaddrDecodingError(<Multiaddr as TryFrom<Vec<u8>>>::Error),
}

impl TryFrom<Vec<u8>> for PeerRecord {
    type Error = DecodeError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        use DecodeError::*;
        let record = peer_record_proto::PeerRecord::decode(value.as_slice())
            .map_err(ProtoBufDecodingError)?;
        let addresses: Result<Vec<Multiaddr>, <Multiaddr as TryFrom<Vec<u8>>>::Error> = record
            .addresses
            .into_iter()
            .map(|address_info| Multiaddr::try_from(address_info.multiaddr))
            .collect();
        Ok(PeerRecord {
            peer_id: PeerId::from_bytes(record.peer_id).map_err(PeerIdDecodingError)?,
            addresses: addresses.map_err(MultiaddrDecodingError)?,
            seq: record.seq,
        })
    }
}

impl Into<Vec<u8>> for PeerRecord {
    fn into(self) -> Vec<u8> {
        let record = peer_record_proto::PeerRecord {
            peer_id: self.peer_id.into_bytes(),
            addresses: self
                .addresses
                .iter()
                .map(|addr| peer_record_proto::peer_record::AddressInfo {
                    multiaddr: addr.to_vec(),
                })
                .collect(),
            seq: self.seq,
        };
        let mut result = Vec::with_capacity(record.encoded_len());
        record
            .encode(&mut result)
            .expect("Vec<u8> provides capacity as needed");
        result
    }
}

impl Record for PeerRecord {
    fn payload_type() -> &'static [u8] {
        "/libp2p/routing-state-record".as_bytes()
    }

    fn domain() -> &'static str {
        "libp2p-routing-state"
    }
}

type SignedPeerRecord = Envelope<PeerRecord>;

pub fn timestamp_seq() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| match d.as_nanos() {
            x if x <= u64::MAX as u128 => u64::try_from(x).ok(),
            x => x
                .checked_rem(u64::MAX as u128)
                .and_then(|x| u64::try_from(x).ok()),
        })
        .unwrap_or_else(|| {
            error!(
                "Couldn't use system time for creating routing record sequence numbers. The \
            system time seems to be before unix epoch."
            );
            0
        })
}

impl PeerRecord {
    fn new(peer_id: PeerId, addresses: Vec<Multiaddr>) -> Self {
        Self {
            peer_id,
            addresses,
            seq: timestamp_seq(),
        }
    }

    fn sign(self, keypair: &Keypair) -> Result<SignedPeerRecord, Error<Self>> {
        SignedPeerRecord::sign(self, keypair)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::multiaddr::Protocol::*;
    use std::iter::FromIterator;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn record() -> PeerRecord {
        PeerRecord::new(
            PeerId::random(),
            vec![
                Multiaddr::from(Ipv4Addr::new(1, 2, 3, 4)),
                Multiaddr::from(Ipv6Addr::new(5, 6, 7, 8, 9, 10, 11, 12)),
                Multiaddr::from_iter(
                    vec![
                        Ip4(Ipv4Addr::new(13, 14, 15, 16)),
                        Ip6(Ipv6Addr::new(17, 18, 19, 20, 21, 22, 23, 24)),
                    ]
                    .into_iter(),
                ),
            ],
        )
    }

    #[test]
    fn test_encoding_decoding_tour() {
        let record = record();
        let vec: Vec<u8> = record.clone().into();

        let record2 = PeerRecord::try_from(vec).unwrap();

        assert_eq!(record, record2);
    }

    #[test]
    fn test_sign_and_verify_record() {
        let record = record();
        let keypair = Keypair::generate_ed25519();
        let envelope = record.sign(&keypair).unwrap();

        assert!(envelope.verify());
    }
}
