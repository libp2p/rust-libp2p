use crate::{META_QUERY_SERVICE, SERVICE_NAME};
use anyhow::Result;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::PeerId;
use std::convert::TryFrom;
use std::io::{Cursor, Read, Write};
use std::str::FromStr;
use std::time::Duration;

/// Maximum size of a DNS label as per RFC1035.
const MAX_LABEL_LENGTH: usize = 63;
/// DNS TXT records can have up to 255 characters as a single string value.
///
/// Current values are usually around 170-190 bytes long, varying primarily
/// with the length of the contained `Multiaddr`.
const MAX_TXT_VALUE_LENGTH: usize = 255;
/// A conservative maximum size (in bytes) of a complete TXT record.
const MAX_TXT_RECORD_SIZE: usize = MAX_TXT_VALUE_LENGTH + 45;
/// The maximum DNS packet size is 9000 bytes less the maximum
/// sizes of the IP (60) and UDP (8) headers.
const MAX_PACKET_SIZE: usize = 9000 - 68;
/// A conservative maximum number of records that can be packed into
/// a single DNS UDP packet, allowing up to 100 bytes of MDNS packet
/// header data to be added.
const MAX_RECORDS_PER_PACKET: usize = (MAX_PACKET_SIZE - 100) / MAX_TXT_RECORD_SIZE;

#[derive(Debug, PartialEq)]
pub enum MdnsPacket {
    PeerRequest {
        query_id: u16,
    },
    PeerResponse {
        query_id: u16,
        peer_id: PeerId,
        addresses: Vec<Multiaddr>,
        ttl: Duration,
    },
    ServiceRequest {
        query_id: u16,
    },
    ServiceResponse {
        query_id: u16,
        ttl: Duration,
    },
}

impl MdnsPacket {
    pub fn to_encoded_packets(&self) -> Vec<Vec<u8>> {
        match self {
            Self::PeerRequest { query_id } => {
                let mut packet = Vec::with_capacity(33);
                // query identifier
                packet.write_u16::<BigEndian>(*query_id).unwrap();
                // 0x0000 indicates that it is a query
                packet.write_u16::<BigEndian>(0x0000).unwrap();
                // number of questions
                packet.write_u16::<BigEndian>(1).unwrap();
                // number of answers
                packet.write_u16::<BigEndian>(0).unwrap();
                // number of authorities
                packet.write_u16::<BigEndian>(0).unwrap();
                // number of additionals
                packet.write_u16::<BigEndian>(0).unwrap();
                // question
                // name
                append_qname(&mut packet, SERVICE_NAME);
                // record type ptr
                packet.write_u16::<BigEndian>(0x000c).unwrap();
                // address class ip
                packet.write_u16::<BigEndian>(0x0001).unwrap();
                debug_assert_eq!(packet.len(), 33);
                vec![packet]
            }
            Self::PeerResponse {
                query_id,
                peer_id,
                addresses,
                ttl,
            } => {
                let ttl = to_ttl(ttl);
                let peer_id_encoded = encode_peer_id(&peer_id);
                debug_assert!(peer_id_encoded.len() + 2 <= 0xffff);
                let mut packets = Vec::with_capacity(addresses.len() % MAX_RECORDS_PER_PACKET);
                for addresses in addresses.chunks(MAX_RECORDS_PER_PACKET) {
                    let mut packet = Vec::with_capacity(addresses.len() * MAX_TXT_RECORD_SIZE);
                    // query identifier
                    packet.write_u16::<BigEndian>(*query_id).unwrap();
                    // 0x8400 indicates that it is a response and it
                    // is an authoritative answer
                    packet.write_u16::<BigEndian>(0x8400).unwrap();
                    // number of questions
                    packet.write_u16::<BigEndian>(0).unwrap();
                    // number of answers
                    packet.write_u16::<BigEndian>(1).unwrap();
                    // number of authorities
                    packet.write_u16::<BigEndian>(0).unwrap();
                    // number of additionals
                    packet
                        .write_u16::<BigEndian>(addresses.len() as u16)
                        .unwrap();
                    // answer
                    // name
                    append_qname(&mut packet, SERVICE_NAME);
                    // record type ptr
                    packet.write_u16::<BigEndian>(0x000c).unwrap();
                    // address class ip
                    packet.write_u16::<BigEndian>(0x0001).unwrap();
                    // ttl
                    packet.write_u32::<BigEndian>(ttl).unwrap();
                    // peer id
                    packet
                        .write_u16::<BigEndian>(peer_id_encoded.len() as u16 + 2)
                        .unwrap();
                    append_qname(&mut packet, &peer_id_encoded);
                    // additionals
                    for address in addresses {
                        let txt = format!("dnsaddr={}/p2p/{}", address, peer_id);
                        assert!(txt.len() <= MAX_TXT_VALUE_LENGTH);
                        // valid multiaddrs don't contain spaces
                        assert!(!txt.bytes().any(|c| c == b' '));
                        append_qname(&mut packet, &peer_id_encoded);
                        // record type txt
                        packet.write_u16::<BigEndian>(0x0010).unwrap();
                        // address class ip with cache-flush bit set
                        packet.write_u16::<BigEndian>(0x8001).unwrap();
                        // ttl
                        packet.write_u32::<BigEndian>(ttl).unwrap();
                        // txt
                        let len = txt.as_bytes().len() as u8;
                        packet.write_u16::<BigEndian>(len as u16 + 1).unwrap();
                        packet.write_all(&[len]).unwrap();
                        packet.extend_from_slice(txt.as_bytes());
                    }
                    packets.push(packet);
                }
                packets
            }
            Self::ServiceRequest { .. } => {
                // Not used by libp2p
                vec![]
            }
            Self::ServiceResponse { query_id, ttl } => {
                let mut packet = Vec::with_capacity(69);
                // query identifier
                packet.write_u16::<BigEndian>(*query_id).unwrap();
                // 0x8400 indicates that it is a response and it
                // is an authoritative answer
                packet.write_u16::<BigEndian>(0x8400).unwrap();
                // number of questions
                packet.write_u16::<BigEndian>(0).unwrap();
                // number of answers
                packet.write_u16::<BigEndian>(1).unwrap();
                // number of authorities
                packet.write_u16::<BigEndian>(0).unwrap();
                // number of additionals
                packet.write_u16::<BigEndian>(0).unwrap();
                // answer
                // name
                append_qname(&mut packet, META_QUERY_SERVICE);
                // record type ptr
                packet.write_u16::<BigEndian>(0x000c).unwrap();
                // address class ip with cache-flush bit set
                packet.write_u16::<BigEndian>(0x8001).unwrap();
                // ttl
                packet.write_u32::<BigEndian>(to_ttl(ttl)).unwrap();
                {
                    let mut name = Vec::with_capacity(SERVICE_NAME.len() + 2);
                    append_qname(&mut name, SERVICE_NAME);
                    packet.write_u16::<BigEndian>(name.len() as u16).unwrap();
                    packet.extend_from_slice(&name);
                }
                debug_assert_eq!(packet.len(), 69);
                vec![packet]
            }
        }
    }

    pub fn from_bytes(packet: &[u8]) -> Result<Self> {
        let mut packet = Cursor::new(packet);
        // query identifier
        let query_id = packet.read_u16::<BigEndian>()?;
        // flag
        let flag = packet.read_u16::<BigEndian>()?;
        // number of questions
        let questions = packet.read_u16::<BigEndian>()? as usize;
        // number of answers
        let answers = packet.read_u16::<BigEndian>()? as usize;
        // number of authorities
        let _authorities = packet.read_u16::<BigEndian>()?;
        // number of additionals
        let additionals = packet.read_u16::<BigEndian>()? as usize;
        // qname
        let qname = read_qname(&mut packet)?;
        match (flag, &qname[..], questions, answers) {
            (0x0000, SERVICE_NAME, 1, 0) => Ok(Self::PeerRequest { query_id }),
            (0x8400, SERVICE_NAME, 0, 1) => {
                let ptr = packet.read_u16::<BigEndian>()?;
                let class = packet.read_u16::<BigEndian>()?;
                if ptr != 0x000c || class != 0x0001 || additionals < 1 {
                    return Err(anyhow::anyhow!("invalid answer"));
                }
                let ttl = Duration::from_secs(packet.read_u32::<BigEndian>()? as u64);
                let _peer_id_len = packet.read_u16::<BigEndian>()? as usize;
                let peer_id_encoded = read_qname(&mut packet)?;
                let peer_id = decode_peer_id(&peer_id_encoded)?;
                let mut addresses = Vec::with_capacity(additionals);
                for _ in 0..additionals {
                    let qname = read_qname(&mut packet)?;
                    let txt = packet.read_u16::<BigEndian>()?;
                    let class = packet.read_u16::<BigEndian>()?;
                    let _ttl = packet.read_u32::<BigEndian>()?;
                    let txt_len = packet.read_u16::<BigEndian>()? as usize;
                    let mut txt_bytes = vec![0; txt_len];
                    packet.read_exact(&mut txt_bytes)?;
                    if txt != 0x0010
                        || class != 0x8001
                        || qname != peer_id_encoded
                        || txt_len < 2
                        || txt_bytes[0] as usize != txt_len - 1
                        || !txt_bytes[1..].starts_with(b"dnsaddr=")
                    {
                        continue;
                    }
                    let addr = if let Ok(addr) = std::str::from_utf8(&txt_bytes[9..]) {
                        addr
                    } else {
                        continue;
                    };
                    let mut addr = if let Ok(addr) = Multiaddr::from_str(addr) {
                        addr
                    } else {
                        continue;
                    };
                    let peer_id2 = match addr.pop() {
                        Some(Protocol::P2p(peer_id2)) => PeerId::try_from(peer_id2).ok(),
                        _ => None,
                    };
                    if Some(peer_id) != peer_id2 {
                        continue;
                    }
                    addresses.push(addr);
                }
                Ok(Self::PeerResponse {
                    query_id,
                    peer_id,
                    addresses,
                    ttl,
                })
            }
            (0x0000, META_QUERY_SERVICE, 1, 0) => Ok(Self::ServiceRequest { query_id }),
            (_, SERVICE_NAME, _, _) | (_, META_QUERY_SERVICE, _, _) => {
                Err(anyhow::anyhow!("invalid mdns packet"))
            }
            (_, qname, _, _) => Err(anyhow::anyhow!("unknown qname {:?}", qname)),
        }
    }
}

/// Appends a `QNAME` (as defined by RFC1035) to the `Vec`.
///
/// # Panic
///
/// Panics if `name` has a zero-length component or a component that is too long.
/// This is fine considering that this function is not public and is only called in a controlled
/// environment.
///
fn append_qname(out: &mut Vec<u8>, name: &[u8]) {
    debug_assert!(name.is_ascii());
    for element in name.split(|&c| c == b'.') {
        assert!(element.len() < 64, "Service name has a label too long");
        assert_ne!(element.len(), 0, "Service name contains zero length label");
        out.push(element.len() as u8);
        for chr in element.iter() {
            out.push(*chr);
        }
    }
    out.push(0);
}

fn read_qname(mut packet: impl Read) -> Result<Vec<u8>> {
    let mut qname = vec![];
    loop {
        let mut byte = [0];
        packet.read_exact(&mut byte)?;
        if byte[0] == 0 {
            break;
        }
        if !qname.is_empty() {
            qname.push(b'.');
        }
        let pos = qname.len();
        qname.resize(pos + byte[0] as usize, 0);
        packet.read_exact(&mut qname[pos..])?;
    }
    Ok(qname)
}

/// Appends a ttl (the number of secs of a duration) to a `Vec`.
fn to_ttl(duration: &Duration) -> u32 {
    let secs = duration
        .as_secs()
        .saturating_add(if duration.subsec_nanos() > 0 { 1 } else { 0 });
    std::cmp::min(secs, From::from(u32::max_value())) as u32
}

/// Combines and encodes a `PeerId` and service name for a DNS query.
fn encode_peer_id(peer_id: &PeerId) -> Vec<u8> {
    // DNS-safe encoding for the Peer ID
    let raw_peer_id = data_encoding::BASE32_DNSCURVE.encode(&peer_id.to_bytes());
    // ensure we don't have any labels over 63 bytes long
    let encoded_peer_id = segment_peer_id(raw_peer_id);
    let mut bytes = Vec::with_capacity(encoded_peer_id.as_bytes().len() + SERVICE_NAME.len() + 1);
    bytes.extend_from_slice(encoded_peer_id.as_bytes());
    bytes.push(b'.');
    bytes.extend_from_slice(SERVICE_NAME);
    bytes
}

fn decode_peer_id(peer_id: &[u8]) -> Result<PeerId> {
    let mut peer_name = peer_id
        .rsplitn(4, |c| *c == b'.')
        .last()
        .ok_or_else(|| anyhow::anyhow!("doesn't end with ._p2p._udp.local"))?
        .to_vec();
    // if we have a segmented name, remove the '.'
    peer_name.retain(|c| *c != b'.');
    let bytes = data_encoding::BASE32_DNSCURVE.decode(&peer_name)?;
    Ok(PeerId::from_bytes(&bytes)?)
}

/// If a peer ID is longer than 63 characters, split it into segments to
/// be compatible with RFC 1035.
fn segment_peer_id(peer_id: String) -> String {
    // Guard for the most common case
    if peer_id.len() <= MAX_LABEL_LENGTH {
        return peer_id;
    }

    // This will only perform one allocation except in extreme circumstances.
    let mut out = String::with_capacity(peer_id.len() + 8);

    for (idx, chr) in peer_id.chars().enumerate() {
        if idx > 0 && idx % MAX_LABEL_LENGTH == 0 {
            out.push('.');
        }
        out.push(chr);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use libp2p_core::identity;
    use std::time::Duration;

    #[test]
    fn peer_request_encode_decode() {
        let query = MdnsPacket::PeerRequest {
            query_id: rand::random(),
        };
        let packets = query.to_encoded_packets();
        assert_eq!(packets.len(), 1);
        let encoded_query = packets.into_iter().next().unwrap();
        let query2 = MdnsPacket::from_bytes(&encoded_query).unwrap();
        assert_eq!(query, query2);
    }

    #[test]
    fn peer_response_encode_decode() {
        let my_peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();
        let addr1 = "/ip4/1.2.3.4/tcp/5000".parse().unwrap();
        let addr2 = "/ip6/::1/udp/10000".parse().unwrap();
        let answer = MdnsPacket::PeerResponse {
            query_id: 0xf8f8,
            peer_id: my_peer_id,
            addresses: vec![addr1, addr2],
            ttl: Duration::from_secs(60),
        };
        let packets = answer.to_encoded_packets();
        assert_eq!(packets.len(), 1);
        let encoded_answer = packets.into_iter().next().unwrap();
        let answer2 = MdnsPacket::from_bytes(&encoded_answer).unwrap();
        assert_eq!(answer, answer2);
    }

    #[test]
    fn service_response_encode() {
        let answer = MdnsPacket::ServiceResponse {
            query_id: rand::random(),
            ttl: Duration::from_secs(120),
        };
        let packets = answer.to_encoded_packets();
        assert_eq!(packets.len(), 1);
    }

    #[test]
    fn peer_id_encode_decode() {
        let my_peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();
        let bytes = encode_peer_id(&my_peer_id);
        let my_peer_id2 = decode_peer_id(&bytes).unwrap();
        assert_eq!(my_peer_id, my_peer_id2);
    }

    #[test]
    fn test_segment_peer_id() {
        let str_32 = String::from_utf8(vec![b'x'; 32]).unwrap();
        let str_63 = String::from_utf8(vec![b'x'; 63]).unwrap();
        let str_64 = String::from_utf8(vec![b'x'; 64]).unwrap();
        let str_126 = String::from_utf8(vec![b'x'; 126]).unwrap();
        let str_127 = String::from_utf8(vec![b'x'; 127]).unwrap();

        assert_eq!(segment_peer_id(str_32.clone()), str_32);
        assert_eq!(segment_peer_id(str_63.clone()), str_63);

        assert_eq!(segment_peer_id(str_64), [&str_63, "x"].join("."));
        assert_eq!(
            segment_peer_id(str_126),
            [&str_63, str_63.as_str()].join(".")
        );
        assert_eq!(segment_peer_id(str_127), [&str_63, &str_63, "x"].join("."));
    }
}
