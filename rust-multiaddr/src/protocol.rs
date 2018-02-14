use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::convert::From;
use std::io::{Cursor, Write, Result as IoResult};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use cid::Cid;
use integer_encoding::{VarInt, VarIntWriter};

use {Result, Error};

///! # ProtocolId
///!
///! A type to describe the possible protocol used in a
///! Multiaddr.

/// ProtocolId is the list of all possible protocols.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u32)]
pub enum ProtocolId {
    IP4 = 4,
    TCP = 6,
    UDP = 17,
    DCCP = 33,
    IP6 = 41,
    DNS4 = 54,
    DNS6 = 55,
    SCTP = 132,
    UDT = 301,
    UTP = 302,
    UNIX = 400,
    P2P = 420,
    IPFS = 421,
    HTTP = 480,
    HTTPS = 443,
    ONION = 444,
    QUIC = 460,
    WS = 477,
    WSS = 478,
    Libp2pWebsocketStar = 479,
    Libp2pWebrtcStar = 275,
    Libp2pWebrtcDirect = 276,
    P2pCircuit = 290,
}

impl From<ProtocolId> for u32 {
    fn from(proto: ProtocolId) -> u32 {
        proto as u32
    }
}

impl From<ProtocolId> for u64 {
    fn from(proto: ProtocolId) -> u64 {
        proto as u32 as u64
    }
}

impl ToString for ProtocolId {
    fn to_string(&self) -> String {
        match *self {
            ProtocolId::IP4 => "ip4",
            ProtocolId::TCP => "tcp",
            ProtocolId::UDP => "udp",
            ProtocolId::DCCP => "dccp",
            ProtocolId::IP6 => "ip6",
            ProtocolId::DNS4 => "dns4",
            ProtocolId::DNS6 => "dns6",
            ProtocolId::SCTP => "sctp",
            ProtocolId::UDT => "udt",
            ProtocolId::UTP => "utp",
            ProtocolId::UNIX => "unix",
            ProtocolId::P2P => "p2p",
            ProtocolId::IPFS => "ipfs",
            ProtocolId::HTTP => "http",
            ProtocolId::HTTPS => "https",
            ProtocolId::ONION => "onion",
            ProtocolId::QUIC => "quic",
            ProtocolId::WS => "ws",
            ProtocolId::WSS => "wss",
            ProtocolId::Libp2pWebsocketStar => "p2p-websocket-star",
            ProtocolId::Libp2pWebrtcStar => "p2p-webrtc-star",
            ProtocolId::Libp2pWebrtcDirect => "p2p-webrtc-direct",
            ProtocolId::P2pCircuit => "p2p-circuit",
        }.to_owned()
    }
}

impl FromStr for ProtocolId {
    type Err = Error;

    fn from_str(raw: &str) -> Result<Self> {
        match raw {
            "ip4" => Ok(ProtocolId::IP4),
            "tcp" => Ok(ProtocolId::TCP),
            "udp" => Ok(ProtocolId::UDP),
            "dccp" => Ok(ProtocolId::DCCP),
            "ip6" => Ok(ProtocolId::IP6),
            "dns4" => Ok(ProtocolId::DNS4),
            "dns6" => Ok(ProtocolId::DNS6),
            "sctp" => Ok(ProtocolId::SCTP),
            "udt" => Ok(ProtocolId::UDT),
            "utp" => Ok(ProtocolId::UTP),
            "unix" => Ok(ProtocolId::UNIX),
            "p2p" => Ok(ProtocolId::P2P),
            "ipfs" => Ok(ProtocolId::IPFS),
            "http" => Ok(ProtocolId::HTTP),
            "https" => Ok(ProtocolId::HTTPS),
            "onion" => Ok(ProtocolId::ONION),
            "quic" => Ok(ProtocolId::QUIC),
            "ws" => Ok(ProtocolId::WS),
            "wss" => Ok(ProtocolId::WSS),
            "p2p-websocket-star" => Ok(ProtocolId::Libp2pWebsocketStar),
            "p2p-webrtc-star" => Ok(ProtocolId::Libp2pWebrtcStar),
            "p2p-webrtc-direct" => Ok(ProtocolId::Libp2pWebrtcDirect),
            "p2p-circuit" => Ok(ProtocolId::P2pCircuit),
            _ => Err(Error::UnknownProtocolString),
        }
    }
}


impl ProtocolId {
    /// Convert a `u64` based code to a `ProtocolId`.
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::ProtocolId;
    ///
    /// assert_eq!(ProtocolId::from(6).unwrap(), ProtocolId::TCP);
    /// assert!(ProtocolId::from(455).is_err());
    /// ```
    pub fn from(raw: u64) -> Result<ProtocolId> {
        match raw {
            4 => Ok(ProtocolId::IP4),
            6 => Ok(ProtocolId::TCP),
            17 => Ok(ProtocolId::UDP),
            33 => Ok(ProtocolId::DCCP),
            41 => Ok(ProtocolId::IP6),
            54 => Ok(ProtocolId::DNS4),
            55 => Ok(ProtocolId::DNS6),
            132 => Ok(ProtocolId::SCTP),
            301 => Ok(ProtocolId::UDT),
            302 => Ok(ProtocolId::UTP),
            400 => Ok(ProtocolId::UNIX),
            420 => Ok(ProtocolId::P2P),
            421 => Ok(ProtocolId::IPFS),
            480 => Ok(ProtocolId::HTTP),
            443 => Ok(ProtocolId::HTTPS),
            444 => Ok(ProtocolId::ONION),
            460 => Ok(ProtocolId::QUIC),
            477 => Ok(ProtocolId::WS),
            478 => Ok(ProtocolId::WSS),
            479 => Ok(ProtocolId::Libp2pWebsocketStar),
            275 => Ok(ProtocolId::Libp2pWebrtcStar),
            276 => Ok(ProtocolId::Libp2pWebrtcDirect),
            290 => Ok(ProtocolId::P2pCircuit),
            _ => Err(Error::UnknownProtocol),
        }
    }

    /// Get the size from a `ProtocolId`.
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::ProtocolId;
    /// use multiaddr::ProtocolArgSize;
    ///
    /// assert_eq!(ProtocolId::TCP.size(), ProtocolArgSize::Fixed { bytes: 2 });
    /// ```
    ///
    pub fn size(&self) -> ProtocolArgSize {
        match *self {
            ProtocolId::IP4 => ProtocolArgSize::Fixed { bytes: 4 },
            ProtocolId::TCP => ProtocolArgSize::Fixed { bytes: 2 },
            ProtocolId::UDP => ProtocolArgSize::Fixed { bytes: 2 },
            ProtocolId::DCCP => ProtocolArgSize::Fixed { bytes: 2 },
            ProtocolId::IP6 => ProtocolArgSize::Fixed { bytes: 16 },
            ProtocolId::DNS4 => ProtocolArgSize::Variable,
            ProtocolId::DNS6 => ProtocolArgSize::Variable,
            ProtocolId::SCTP => ProtocolArgSize::Fixed { bytes: 2 },
            ProtocolId::UDT => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::UTP => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::UNIX => ProtocolArgSize::Variable,
            ProtocolId::P2P => ProtocolArgSize::Variable,
            ProtocolId::IPFS => ProtocolArgSize::Variable,
            ProtocolId::HTTP => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::HTTPS => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::ONION => ProtocolArgSize::Fixed { bytes: 10 },
            ProtocolId::QUIC => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::WS => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::WSS => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::Libp2pWebsocketStar => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::Libp2pWebrtcStar => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::Libp2pWebrtcDirect => ProtocolArgSize::Fixed { bytes: 0 },
            ProtocolId::P2pCircuit => ProtocolArgSize::Fixed { bytes: 0 },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProtocolArgSize {
    /// The size of the argument is of fixed length. The length can be 0, in which case there is no
    /// argument.
    Fixed { bytes: usize },
    /// The size of the argument is of variable length.
    Variable,
}

impl ProtocolId {
    /// Convert an array slice to the string representation.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::net::Ipv4Addr;
    /// use multiaddr::AddrComponent;
    /// use multiaddr::ProtocolId;
    ///
    /// let proto = ProtocolId::IP4;
    /// assert_eq!(proto.parse_data("127.0.0.1").unwrap(),
    ///            AddrComponent::IP4(Ipv4Addr::new(127, 0, 0, 1)));
    /// ```
    ///
    pub fn parse_data(&self, a: &str) -> Result<AddrComponent> {
        match *self {
            ProtocolId::IP4 => {
                let addr = Ipv4Addr::from_str(a)?;
                Ok(AddrComponent::IP4(addr))
            }
            ProtocolId::IP6 => {
                let addr = Ipv6Addr::from_str(a)?;
                Ok(AddrComponent::IP6(addr))
            }
            ProtocolId::DNS4 => {
                Ok(AddrComponent::DNS4(a.to_owned()))
            }
            ProtocolId::DNS6 => {
                Ok(AddrComponent::DNS6(a.to_owned()))
            }
            ProtocolId::TCP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::TCP(parsed))
            }
            ProtocolId::UDP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::UDP(parsed))
            }
            ProtocolId::DCCP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::DCCP(parsed))
            }
            ProtocolId::SCTP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::SCTP(parsed))
            }
            ProtocolId::P2P => {
                let bytes = Cid::from(a)?.to_bytes();
                Ok(AddrComponent::P2P(bytes))
            }
            ProtocolId::IPFS => {
                let bytes = Cid::from(a)?.to_bytes();
                Ok(AddrComponent::IPFS(bytes))
            }
            ProtocolId::ONION => unimplemented!(),              // TODO:
            ProtocolId::QUIC => Ok(AddrComponent::QUIC),
            ProtocolId::UTP => Ok(AddrComponent::UTP),
            ProtocolId::UNIX => {
                Ok(AddrComponent::UNIX(a.to_owned()))
            }
            ProtocolId::UDT => Ok(AddrComponent::UDT),
            ProtocolId::HTTP => Ok(AddrComponent::HTTP),
            ProtocolId::HTTPS => Ok(AddrComponent::HTTPS),
            ProtocolId::WS => Ok(AddrComponent::WS),
            ProtocolId::WSS => Ok(AddrComponent::WSS),
            ProtocolId::Libp2pWebsocketStar => Ok(AddrComponent::Libp2pWebsocketStar),
            ProtocolId::Libp2pWebrtcStar => Ok(AddrComponent::Libp2pWebrtcStar),
            ProtocolId::Libp2pWebrtcDirect => Ok(AddrComponent::Libp2pWebrtcDirect),
            ProtocolId::P2pCircuit => Ok(AddrComponent::P2pCircuit),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum AddrComponent {
    IP4(Ipv4Addr),
    TCP(u16),
    UDP(u16),
    DCCP(u16),
    IP6(Ipv6Addr),
    DNS4(String),
    DNS6(String),
    SCTP(u16),
    UDT,
    UTP,
    UNIX(String),
    P2P(Vec<u8>),
    IPFS(Vec<u8>),
    HTTP,
    HTTPS,
    ONION(Vec<u8>),
    QUIC,
    WS,
    WSS,
    Libp2pWebsocketStar,
    Libp2pWebrtcStar,
    Libp2pWebrtcDirect,
    P2pCircuit,
}

impl AddrComponent {
    /// Returns the `ProtocolId` corresponding to this `AddrComponent`.
    #[inline]
    pub fn protocol_id(&self) -> ProtocolId {
        match *self {
            AddrComponent::IP4(_) => ProtocolId::IP4,
            AddrComponent::TCP(_) => ProtocolId::TCP,
            AddrComponent::UDP(_) => ProtocolId::UDP,
            AddrComponent::DCCP(_) => ProtocolId::DCCP,
            AddrComponent::IP6(_) => ProtocolId::IP6,
            AddrComponent::DNS4(_) => ProtocolId::DNS4,
            AddrComponent::DNS6(_) => ProtocolId::DNS6,
            AddrComponent::SCTP(_) => ProtocolId::SCTP,
            AddrComponent::UDT => ProtocolId::UDT,
            AddrComponent::UTP => ProtocolId::UTP,
            AddrComponent::UNIX(_) => ProtocolId::UNIX,
            AddrComponent::P2P(_) => ProtocolId::P2P,
            AddrComponent::IPFS(_) => ProtocolId::IPFS,
            AddrComponent::HTTP => ProtocolId::HTTP,
            AddrComponent::HTTPS => ProtocolId::HTTPS,
            AddrComponent::ONION(_) => ProtocolId::ONION,
            AddrComponent::QUIC => ProtocolId::QUIC,
            AddrComponent::WS => ProtocolId::WS,
            AddrComponent::WSS => ProtocolId::WSS,
            AddrComponent::Libp2pWebsocketStar => ProtocolId::Libp2pWebsocketStar,
            AddrComponent::Libp2pWebrtcStar => ProtocolId::Libp2pWebrtcStar,
            AddrComponent::Libp2pWebrtcDirect => ProtocolId::Libp2pWebrtcDirect,
            AddrComponent::P2pCircuit => ProtocolId::P2pCircuit,
        }
    }

    pub fn from_bytes(input: &[u8]) -> Result<(AddrComponent, &[u8])> {
        let (proto_num, proto_id_len) = u64::decode_var(input);   // TODO: will panic if ID too large

        let protocol_id = ProtocolId::from(proto_num)?;
        let (data_offset, data_size) = match protocol_id.size() {
            ProtocolArgSize::Fixed { bytes } => {
                (0, bytes)
            },
            ProtocolArgSize::Variable => {
                let (data_size, varint_len) = u64::decode_var(&input[proto_id_len..]);      // TODO: will panic if ID too large
                (varint_len, data_size as usize)
            },
        };

        let (data, rest) = input[proto_id_len..][data_offset..].split_at(data_size);

        let addr_component = match protocol_id {
            ProtocolId::IP4 => {
                AddrComponent::IP4(Ipv4Addr::new(data[0], data[1], data[2], data[3]))
            },
            ProtocolId::IP6 => {
                let mut rdr = Cursor::new(data);
                let mut seg = vec![];

                for _ in 0..8 {
                    seg.push(rdr.read_u16::<BigEndian>()?);
                }

                let addr = Ipv6Addr::new(seg[0],
                                         seg[1],
                                         seg[2],
                                         seg[3],
                                         seg[4],
                                         seg[5],
                                         seg[6],
                                         seg[7]);
                AddrComponent::IP6(addr)
            }
            ProtocolId::DNS4 => {
                AddrComponent::DNS4(String::from_utf8(data.to_owned())?)
            }
            ProtocolId::DNS6 => {
                AddrComponent::DNS6(String::from_utf8(data.to_owned())?)
            }
            ProtocolId::TCP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::TCP(num)
            }
            ProtocolId::UDP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::UDP(num)
            }
            ProtocolId::DCCP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::DCCP(num)
            }
            ProtocolId::SCTP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::SCTP(num)
            }
            ProtocolId::UNIX => {
                AddrComponent::UNIX(String::from_utf8(data.to_owned())?)
            }
            ProtocolId::P2P => {
                let bytes = Cid::from(data)?.to_bytes();
                AddrComponent::P2P(bytes)
            }
            ProtocolId::IPFS => {
                let bytes = Cid::from(data)?.to_bytes();
                AddrComponent::IPFS(bytes)
            }
            ProtocolId::ONION => unimplemented!(),      // TODO:
            ProtocolId::QUIC => AddrComponent::QUIC,
            ProtocolId::UTP => AddrComponent::UTP,
            ProtocolId::UDT => AddrComponent::UDT,
            ProtocolId::HTTP => AddrComponent::HTTP,
            ProtocolId::HTTPS => AddrComponent::HTTPS,
            ProtocolId::WS => AddrComponent::WS,
            ProtocolId::WSS => AddrComponent::WSS,
            ProtocolId::Libp2pWebsocketStar => AddrComponent::Libp2pWebsocketStar,
            ProtocolId::Libp2pWebrtcStar => AddrComponent::Libp2pWebrtcStar,
            ProtocolId::Libp2pWebrtcDirect => AddrComponent::Libp2pWebrtcDirect,
            ProtocolId::P2pCircuit => AddrComponent::P2pCircuit,
        };

        Ok((addr_component, rest))
    }

    /// Turns this address component into bytes by writing it to a `Write`.
    pub fn write_bytes<W: Write>(self, out: &mut W) -> IoResult<()> {
        out.write_varint(Into::<u64>::into(self.protocol_id()))?;

        match self {
            AddrComponent::IP4(addr) => {
                out.write_all(&addr.octets())?;
            }
            AddrComponent::IP6(addr) => {
                for &segment in &addr.segments() {
                    out.write_u16::<BigEndian>(segment)?;
                }
            }
            AddrComponent::TCP(port) | AddrComponent::UDP(port) | AddrComponent::DCCP(port) |
            AddrComponent::SCTP(port) => {
                out.write_u16::<BigEndian>(port)?;
            }
            AddrComponent::DNS4(s) | AddrComponent::DNS6(s) | AddrComponent::UNIX(s) => {
                let bytes = s.as_bytes();
                out.write_varint(bytes.len())?;
                out.write_all(&bytes)?;
            }
            AddrComponent::P2P(bytes) | AddrComponent::IPFS(bytes) => {
                out.write_varint(bytes.len())?;
                out.write_all(&bytes)?;
            }
            AddrComponent::ONION(_) => {
                unimplemented!()  // TODO:
            },
            AddrComponent::QUIC |
            AddrComponent::UTP |
            AddrComponent::UDT |
            AddrComponent::HTTP |
            AddrComponent::HTTPS |
            AddrComponent::WS |
            AddrComponent::WSS |
            AddrComponent::Libp2pWebsocketStar |
            AddrComponent::Libp2pWebrtcStar |
            AddrComponent::Libp2pWebrtcDirect |
            AddrComponent::P2pCircuit => {}
        };

        Ok(())
    }
}

impl ToString for AddrComponent {
    fn to_string(&self) -> String {
        match *self {
            AddrComponent::IP4(ref addr) => format!("/ip4/{}", addr),
            AddrComponent::TCP(port) => format!("/tcp/{}", port),
            AddrComponent::UDP(port) => format!("/udp/{}", port),
            AddrComponent::DCCP(port) => format!("/dccp/{}", port),
            AddrComponent::IP6(ref addr) => format!("/ip6/{}", addr),
            AddrComponent::DNS4(ref s) => format!("/dns4/{}", s.clone()),
            AddrComponent::DNS6(ref s) => format!("/dns6/{}", s.clone()),
            AddrComponent::SCTP(port) => format!("/sctp/{}", port),
            AddrComponent::UDT => format!("/udt"),
            AddrComponent::UTP => format!("/utp"),
            AddrComponent::UNIX(ref s) => format!("/unix/{}", s.clone()),
            AddrComponent::P2P(ref bytes) => {
                // TODO: meh for cloning
                let c = Cid::from(bytes.clone()).expect("cid is known to be valid");
                format!("/p2p/{}", c)
            },
            AddrComponent::IPFS(ref bytes) => {
                // TODO: meh for cloning
                let c = Cid::from(bytes.clone()).expect("cid is known to be valid");
                format!("/ipfs/{}", c)
            },
            AddrComponent::HTTP => format!("/http"),
            AddrComponent::HTTPS => format!("/https"),
            AddrComponent::ONION(_) => unimplemented!(),//format!("/onion"),        // TODO:
            AddrComponent::QUIC => format!("/quic"),
            AddrComponent::WS => format!("/ws"),
            AddrComponent::WSS => format!("/wss"),
            AddrComponent::Libp2pWebsocketStar => format!("/p2p-websocket-star"),
            AddrComponent::Libp2pWebrtcStar => format!("/p2p-webrtc-star"),
            AddrComponent::Libp2pWebrtcDirect => format!("/p2p-webrtc-direct"),
            AddrComponent::P2pCircuit => format!("/p2p-circuit"),
        }
    }
}
