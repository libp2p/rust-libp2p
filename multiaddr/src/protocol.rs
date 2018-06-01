use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::convert::From;
use std::io::{Cursor, Write, Result as IoResult};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use cid::Cid;
use integer_encoding::{VarInt, VarIntWriter};

use {Result, Error};

///! # Protocol
///!
///! A type to describe the possible protocol used in a
///! Multiaddr.

/// Protocol is the list of all possible protocols.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u32)]
pub enum Protocol {
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

impl From<Protocol> for u32 {
    fn from(proto: Protocol) -> u32 {
        proto as u32
    }
}

impl From<Protocol> for u64 {
    fn from(proto: Protocol) -> u64 {
        proto as u32 as u64
    }
}

impl ToString for Protocol {
    fn to_string(&self) -> String {
        match *self {
            Protocol::IP4 => "ip4",
            Protocol::TCP => "tcp",
            Protocol::UDP => "udp",
            Protocol::DCCP => "dccp",
            Protocol::IP6 => "ip6",
            Protocol::DNS4 => "dns4",
            Protocol::DNS6 => "dns6",
            Protocol::SCTP => "sctp",
            Protocol::UDT => "udt",
            Protocol::UTP => "utp",
            Protocol::UNIX => "unix",
            Protocol::P2P => "p2p",
            Protocol::IPFS => "ipfs",
            Protocol::HTTP => "http",
            Protocol::HTTPS => "https",
            Protocol::ONION => "onion",
            Protocol::QUIC => "quic",
            Protocol::WS => "ws",
            Protocol::WSS => "wss",
            Protocol::Libp2pWebsocketStar => "p2p-websocket-star",
            Protocol::Libp2pWebrtcStar => "p2p-webrtc-star",
            Protocol::Libp2pWebrtcDirect => "p2p-webrtc-direct",
            Protocol::P2pCircuit => "p2p-circuit",
        }.to_owned()
    }
}

impl FromStr for Protocol {
    type Err = Error;

    fn from_str(raw: &str) -> Result<Self> {
        match raw {
            "ip4" => Ok(Protocol::IP4),
            "tcp" => Ok(Protocol::TCP),
            "udp" => Ok(Protocol::UDP),
            "dccp" => Ok(Protocol::DCCP),
            "ip6" => Ok(Protocol::IP6),
            "dns4" => Ok(Protocol::DNS4),
            "dns6" => Ok(Protocol::DNS6),
            "sctp" => Ok(Protocol::SCTP),
            "udt" => Ok(Protocol::UDT),
            "utp" => Ok(Protocol::UTP),
            "unix" => Ok(Protocol::UNIX),
            "p2p" => Ok(Protocol::P2P),
            "ipfs" => Ok(Protocol::IPFS),
            "http" => Ok(Protocol::HTTP),
            "https" => Ok(Protocol::HTTPS),
            "onion" => Ok(Protocol::ONION),
            "quic" => Ok(Protocol::QUIC),
            "ws" => Ok(Protocol::WS),
            "wss" => Ok(Protocol::WSS),
            "p2p-websocket-star" => Ok(Protocol::Libp2pWebsocketStar),
            "p2p-webrtc-star" => Ok(Protocol::Libp2pWebrtcStar),
            "p2p-webrtc-direct" => Ok(Protocol::Libp2pWebrtcDirect),
            "p2p-circuit" => Ok(Protocol::P2pCircuit),
            _ => Err(Error::UnknownProtocolString),
        }
    }
}


impl Protocol {
    /// Convert a `u64` based code to a `Protocol`.
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::Protocol;
    ///
    /// assert_eq!(Protocol::from(6).unwrap(), Protocol::TCP);
    /// assert!(Protocol::from(455).is_err());
    /// ```
    pub fn from(raw: u64) -> Result<Protocol> {
        match raw {
            4 => Ok(Protocol::IP4),
            6 => Ok(Protocol::TCP),
            17 => Ok(Protocol::UDP),
            33 => Ok(Protocol::DCCP),
            41 => Ok(Protocol::IP6),
            54 => Ok(Protocol::DNS4),
            55 => Ok(Protocol::DNS6),
            132 => Ok(Protocol::SCTP),
            301 => Ok(Protocol::UDT),
            302 => Ok(Protocol::UTP),
            400 => Ok(Protocol::UNIX),
            420 => Ok(Protocol::P2P),
            421 => Ok(Protocol::IPFS),
            480 => Ok(Protocol::HTTP),
            443 => Ok(Protocol::HTTPS),
            444 => Ok(Protocol::ONION),
            460 => Ok(Protocol::QUIC),
            477 => Ok(Protocol::WS),
            478 => Ok(Protocol::WSS),
            479 => Ok(Protocol::Libp2pWebsocketStar),
            275 => Ok(Protocol::Libp2pWebrtcStar),
            276 => Ok(Protocol::Libp2pWebrtcDirect),
            290 => Ok(Protocol::P2pCircuit),
            _ => Err(Error::UnknownProtocol),
        }
    }

    /// Get the size from a `Protocol`.
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::Protocol;
    /// use multiaddr::ProtocolArgSize;
    ///
    /// assert_eq!(Protocol::TCP.size(), ProtocolArgSize::Fixed { bytes: 2 });
    /// ```
    ///
    pub fn size(&self) -> ProtocolArgSize {
        match *self {
            Protocol::IP4 => ProtocolArgSize::Fixed { bytes: 4 },
            Protocol::TCP => ProtocolArgSize::Fixed { bytes: 2 },
            Protocol::UDP => ProtocolArgSize::Fixed { bytes: 2 },
            Protocol::DCCP => ProtocolArgSize::Fixed { bytes: 2 },
            Protocol::IP6 => ProtocolArgSize::Fixed { bytes: 16 },
            Protocol::DNS4 => ProtocolArgSize::Variable,
            Protocol::DNS6 => ProtocolArgSize::Variable,
            Protocol::SCTP => ProtocolArgSize::Fixed { bytes: 2 },
            Protocol::UDT => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::UTP => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::UNIX => ProtocolArgSize::Variable,
            Protocol::P2P => ProtocolArgSize::Variable,
            Protocol::IPFS => ProtocolArgSize::Variable,
            Protocol::HTTP => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::HTTPS => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::ONION => ProtocolArgSize::Fixed { bytes: 10 },
            Protocol::QUIC => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::WS => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::WSS => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::Libp2pWebsocketStar => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::Libp2pWebrtcStar => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::Libp2pWebrtcDirect => ProtocolArgSize::Fixed { bytes: 0 },
            Protocol::P2pCircuit => ProtocolArgSize::Fixed { bytes: 0 },
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

impl Protocol {
    /// Convert an array slice to the string representation.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::net::Ipv4Addr;
    /// use multiaddr::AddrComponent;
    /// use multiaddr::Protocol;
    ///
    /// let proto = Protocol::IP4;
    /// assert_eq!(proto.parse_data("127.0.0.1").unwrap(),
    ///            AddrComponent::IP4(Ipv4Addr::new(127, 0, 0, 1)));
    /// ```
    ///
    pub fn parse_data(&self, a: &str) -> Result<AddrComponent> {
        match *self {
            Protocol::IP4 => {
                let addr = Ipv4Addr::from_str(a)?;
                Ok(AddrComponent::IP4(addr))
            }
            Protocol::IP6 => {
                let addr = Ipv6Addr::from_str(a)?;
                Ok(AddrComponent::IP6(addr))
            }
            Protocol::DNS4 => {
                Ok(AddrComponent::DNS4(a.to_owned()))
            }
            Protocol::DNS6 => {
                Ok(AddrComponent::DNS6(a.to_owned()))
            }
            Protocol::TCP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::TCP(parsed))
            }
            Protocol::UDP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::UDP(parsed))
            }
            Protocol::DCCP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::DCCP(parsed))
            }
            Protocol::SCTP => {
                let parsed: u16 = a.parse()?;
                Ok(AddrComponent::SCTP(parsed))
            }
            Protocol::P2P => {
                let bytes = Cid::from(a)?.to_bytes();
                Ok(AddrComponent::P2P(bytes))
            }
            Protocol::IPFS => {
                let bytes = Cid::from(a)?.to_bytes();
                Ok(AddrComponent::IPFS(bytes))
            }
            Protocol::ONION => unimplemented!(),              // TODO:
            Protocol::QUIC => Ok(AddrComponent::QUIC),
            Protocol::UTP => Ok(AddrComponent::UTP),
            Protocol::UNIX => {
                Ok(AddrComponent::UNIX(a.to_owned()))
            }
            Protocol::UDT => Ok(AddrComponent::UDT),
            Protocol::HTTP => Ok(AddrComponent::HTTP),
            Protocol::HTTPS => Ok(AddrComponent::HTTPS),
            Protocol::WS => Ok(AddrComponent::WS),
            Protocol::WSS => Ok(AddrComponent::WSS),
            Protocol::Libp2pWebsocketStar => Ok(AddrComponent::Libp2pWebsocketStar),
            Protocol::Libp2pWebrtcStar => Ok(AddrComponent::Libp2pWebrtcStar),
            Protocol::Libp2pWebrtcDirect => Ok(AddrComponent::Libp2pWebrtcDirect),
            Protocol::P2pCircuit => Ok(AddrComponent::P2pCircuit),
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
    /// Returns the `Protocol` corresponding to this `AddrComponent`.
    #[inline]
    pub fn protocol_id(&self) -> Protocol {
        match *self {
            AddrComponent::IP4(_) => Protocol::IP4,
            AddrComponent::TCP(_) => Protocol::TCP,
            AddrComponent::UDP(_) => Protocol::UDP,
            AddrComponent::DCCP(_) => Protocol::DCCP,
            AddrComponent::IP6(_) => Protocol::IP6,
            AddrComponent::DNS4(_) => Protocol::DNS4,
            AddrComponent::DNS6(_) => Protocol::DNS6,
            AddrComponent::SCTP(_) => Protocol::SCTP,
            AddrComponent::UDT => Protocol::UDT,
            AddrComponent::UTP => Protocol::UTP,
            AddrComponent::UNIX(_) => Protocol::UNIX,
            AddrComponent::P2P(_) => Protocol::P2P,
            AddrComponent::IPFS(_) => Protocol::IPFS,
            AddrComponent::HTTP => Protocol::HTTP,
            AddrComponent::HTTPS => Protocol::HTTPS,
            AddrComponent::ONION(_) => Protocol::ONION,
            AddrComponent::QUIC => Protocol::QUIC,
            AddrComponent::WS => Protocol::WS,
            AddrComponent::WSS => Protocol::WSS,
            AddrComponent::Libp2pWebsocketStar => Protocol::Libp2pWebsocketStar,
            AddrComponent::Libp2pWebrtcStar => Protocol::Libp2pWebrtcStar,
            AddrComponent::Libp2pWebrtcDirect => Protocol::Libp2pWebrtcDirect,
            AddrComponent::P2pCircuit => Protocol::P2pCircuit,
        }
    }

    /// Builds an `AddrComponent` from an array that starts with a bytes representation. On
    /// success, also returns the rest of the slice.
    pub fn from_bytes(input: &[u8]) -> Result<(AddrComponent, &[u8])> {
        let (proto_num, proto_id_len) = u64::decode_var(input);   // TODO: will panic if ID too large

        let protocol_id = Protocol::from(proto_num)?;
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
            Protocol::IP4 => {
                AddrComponent::IP4(Ipv4Addr::new(data[0], data[1], data[2], data[3]))
            },
            Protocol::IP6 => {
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
            Protocol::DNS4 => {
                AddrComponent::DNS4(String::from_utf8(data.to_owned())?)
            }
            Protocol::DNS6 => {
                AddrComponent::DNS6(String::from_utf8(data.to_owned())?)
            }
            Protocol::TCP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::TCP(num)
            }
            Protocol::UDP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::UDP(num)
            }
            Protocol::DCCP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::DCCP(num)
            }
            Protocol::SCTP => {
                let mut rdr = Cursor::new(data);
                let num = rdr.read_u16::<BigEndian>()?;
                AddrComponent::SCTP(num)
            }
            Protocol::UNIX => {
                AddrComponent::UNIX(String::from_utf8(data.to_owned())?)
            }
            Protocol::P2P => {
                let bytes = Cid::from(data)?.to_bytes();
                AddrComponent::P2P(bytes)
            }
            Protocol::IPFS => {
                let bytes = Cid::from(data)?.to_bytes();
                AddrComponent::IPFS(bytes)
            }
            Protocol::ONION => unimplemented!(),      // TODO:
            Protocol::QUIC => AddrComponent::QUIC,
            Protocol::UTP => AddrComponent::UTP,
            Protocol::UDT => AddrComponent::UDT,
            Protocol::HTTP => AddrComponent::HTTP,
            Protocol::HTTPS => AddrComponent::HTTPS,
            Protocol::WS => AddrComponent::WS,
            Protocol::WSS => AddrComponent::WSS,
            Protocol::Libp2pWebsocketStar => AddrComponent::Libp2pWebsocketStar,
            Protocol::Libp2pWebrtcStar => AddrComponent::Libp2pWebrtcStar,
            Protocol::Libp2pWebrtcDirect => AddrComponent::Libp2pWebrtcDirect,
            Protocol::P2pCircuit => AddrComponent::P2pCircuit,
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
