use bs58;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::{
    borrow::Cow,
    convert::From,
    fmt,
    io::{Cursor, Write, Result as IoResult},
    net::{Ipv4Addr, Ipv6Addr},
    str::{self, FromStr}
};
use multihash::Multihash;
use unsigned_varint::{encode, decode};
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
    P2P = 421,
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
    Memory = 777,       // TODO: not standard: https://github.com/multiformats/multiaddr/pull/71
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

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Protocol::IP4 => f.write_str("ip4"),
            Protocol::TCP => f.write_str("tcp"),
            Protocol::UDP => f.write_str("udp"),
            Protocol::DCCP => f.write_str("dccp"),
            Protocol::IP6 => f.write_str("ip6"),
            Protocol::DNS4 => f.write_str("dns4"),
            Protocol::DNS6 => f.write_str("dns6"),
            Protocol::SCTP => f.write_str("sctp"),
            Protocol::UDT => f.write_str("udt"),
            Protocol::UTP => f.write_str("utp"),
            Protocol::UNIX => f.write_str("unix"),
            Protocol::P2P => f.write_str("p2p"),
            Protocol::HTTP => f.write_str("http"),
            Protocol::HTTPS => f.write_str("https"),
            Protocol::ONION => f.write_str("onion"),
            Protocol::QUIC => f.write_str("quic"),
            Protocol::WS => f.write_str("ws"),
            Protocol::WSS => f.write_str("wss"),
            Protocol::Libp2pWebsocketStar => f.write_str("p2p-websocket-star"),
            Protocol::Libp2pWebrtcStar => f.write_str("p2p-webrtc-star"),
            Protocol::Libp2pWebrtcDirect => f.write_str("p2p-webrtc-direct"),
            Protocol::P2pCircuit => f.write_str("p2p-circuit"),
            Protocol::Memory => f.write_str("memory"),
        }
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
            "memory" => Ok(Protocol::Memory),
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
            421 => Ok(Protocol::P2P),
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
            777 => Ok(Protocol::Memory),
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
            Protocol::Memory => ProtocolArgSize::Fixed { bytes: 0 },
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
    pub fn parse_data<'a>(&self, a: &'a str) -> Result<AddrComponent<'a>> {
        match *self {
            Protocol::IP4 => {
                let addr = Ipv4Addr::from_str(a)?;
                Ok(AddrComponent::IP4(addr))
            }
            Protocol::IP6 => {
                let addr = Ipv6Addr::from_str(a)?;
                Ok(AddrComponent::IP6(addr))
            }
            Protocol::DNS4 => Ok(AddrComponent::DNS4(Cow::Borrowed(a))),
            Protocol::DNS6 => Ok(AddrComponent::DNS6(Cow::Borrowed(a))),
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
                let decoded = bs58::decode(a).into_vec()?;
                Ok(AddrComponent::P2P(Multihash::from_bytes(decoded)?))
            }
            Protocol::ONION => unimplemented!(),              // TODO:
            Protocol::QUIC => Ok(AddrComponent::QUIC),
            Protocol::UTP => Ok(AddrComponent::UTP),
            Protocol::UNIX => Ok(AddrComponent::UNIX(Cow::Borrowed(a))),
            Protocol::UDT => Ok(AddrComponent::UDT),
            Protocol::HTTP => Ok(AddrComponent::HTTP),
            Protocol::HTTPS => Ok(AddrComponent::HTTPS),
            Protocol::WS => Ok(AddrComponent::WS),
            Protocol::WSS => Ok(AddrComponent::WSS),
            Protocol::Libp2pWebsocketStar => Ok(AddrComponent::Libp2pWebsocketStar),
            Protocol::Libp2pWebrtcStar => Ok(AddrComponent::Libp2pWebrtcStar),
            Protocol::Libp2pWebrtcDirect => Ok(AddrComponent::Libp2pWebrtcDirect),
            Protocol::P2pCircuit => Ok(AddrComponent::P2pCircuit),
            Protocol::Memory => Ok(AddrComponent::Memory),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum AddrComponent<'a> {
    IP4(Ipv4Addr),
    TCP(u16),
    UDP(u16),
    DCCP(u16),
    IP6(Ipv6Addr),
    DNS4(Cow<'a, str>),
    DNS6(Cow<'a, str>),
    SCTP(u16),
    UDT,
    UTP,
    UNIX(Cow<'a, str>),
    P2P(Multihash),
    HTTP,
    HTTPS,
    ONION(Cow<'a, [u8]>),
    QUIC,
    WS,
    WSS,
    Libp2pWebsocketStar,
    Libp2pWebrtcStar,
    Libp2pWebrtcDirect,
    P2pCircuit,
    Memory,
}

impl<'a> AddrComponent<'a> {
    /// Turn this `AddrComponent` into one that owns its data, thus being valid for any lifetime.
    pub fn acquire<'b>(self) -> AddrComponent<'b> {
        use AddrComponent::*;
        match self {
            DNS4(cow) => DNS4(Cow::Owned(cow.into_owned())),
            DNS6(cow) => DNS6(Cow::Owned(cow.into_owned())),
            UNIX(cow) => UNIX(Cow::Owned(cow.into_owned())),
            ONION(cow) => ONION(Cow::Owned(cow.into_owned())),
            IP4(a) => IP4(a),
            TCP(a) => TCP(a),
            UDP(a) => UDP(a),
            DCCP(a) => DCCP(a),
            IP6(a) => IP6(a),
            SCTP(a) => SCTP(a),
            UDT => UDT,
            UTP => UTP,
            P2P(a) => P2P(a),
            HTTP => HTTP,
            HTTPS => HTTPS,
            QUIC => QUIC,
            WS => WS,
            WSS => WSS,
            Libp2pWebsocketStar => Libp2pWebsocketStar,
            Libp2pWebrtcStar => Libp2pWebrtcStar,
            Libp2pWebrtcDirect => Libp2pWebrtcDirect,
            P2pCircuit => P2pCircuit,
            Memory => Memory
        }
    }

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
            AddrComponent::Memory => Protocol::Memory,
        }
    }

    /// Builds an `AddrComponent` from an array that starts with a bytes representation. On
    /// success, also returns the rest of the slice.
    pub fn from_bytes(input: &[u8]) -> Result<(AddrComponent, &[u8])> {
        let (proto_num, input) = decode::u64(input)?;
        let protocol_id = Protocol::from(proto_num)?;
        let (data_size, input) = match protocol_id.size() {
            ProtocolArgSize::Fixed { bytes } => (bytes, input),
            ProtocolArgSize::Variable => decode::usize(input)?
        };
        let (data, rest) = input.split_at(data_size);

        let addr_component = match protocol_id {
            Protocol::IP4 => {
                AddrComponent::IP4(Ipv4Addr::new(data[0], data[1], data[2], data[3]))
            },
            Protocol::IP6 => {
                let mut rdr = Cursor::new(data);
                let mut seg = [0; 8];

                for i in 0..8 {
                    seg[i] = rdr.read_u16::<BigEndian>()?;
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
                AddrComponent::DNS4(Cow::Borrowed(str::from_utf8(data)?))
            }
            Protocol::DNS6 => {
                AddrComponent::DNS6(Cow::Borrowed(str::from_utf8(data)?))
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
                AddrComponent::UNIX(Cow::Borrowed(str::from_utf8(data)?))
            }
            Protocol::P2P => {
                AddrComponent::P2P(Multihash::from_bytes(data.to_owned())?)
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
            Protocol::Memory => AddrComponent::Memory,
        };

        Ok((addr_component, rest))
    }

    /// Turns this address component into bytes by writing it to a `Write`.
    pub fn write_bytes<W: Write>(self, out: &mut W) -> IoResult<()> {
        out.write_all(encode::u64(self.protocol_id().into(), &mut encode::u64_buffer()))?;

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
                out.write_all(encode::usize(bytes.len(), &mut encode::usize_buffer()))?;
                out.write_all(&bytes)?;
            }
            AddrComponent::P2P(multihash) => {
                let bytes = multihash.into_bytes();
                out.write_all(encode::usize(bytes.len(), &mut encode::usize_buffer()))?;
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
            AddrComponent::P2pCircuit |
            AddrComponent::Memory => {}
        };

        Ok(())
    }
}

impl<'a> fmt::Display for AddrComponent<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AddrComponent::IP4(addr) => write!(f, "/ip4/{}", addr),
            AddrComponent::TCP(port) => write!(f, "/tcp/{}", port),
            AddrComponent::UDP(port) => write!(f, "/udp/{}", port),
            AddrComponent::DCCP(port) => write!(f, "/dccp/{}", port),
            AddrComponent::IP6(addr) => write!(f, "/ip6/{}", addr),
            AddrComponent::DNS4(s) => write!(f, "/dns4/{}", s),
            AddrComponent::DNS6(s) => write!(f, "/dns6/{}", s),
            AddrComponent::SCTP(port) => write!(f, "/sctp/{}", port),
            AddrComponent::UDT => f.write_str("/udt"),
            AddrComponent::UTP => f.write_str("/utp"),
            AddrComponent::UNIX(s) => write!(f, "/unix/{}", s),
            AddrComponent::P2P(c) => write!(f, "/p2p/{}", bs58::encode(c.as_bytes()).into_string()),
            AddrComponent::HTTP => f.write_str("/http"),
            AddrComponent::HTTPS => f.write_str("/https"),
            AddrComponent::ONION(_) => unimplemented!(),//write!("/onion"),        // TODO:
            AddrComponent::QUIC => f.write_str("/quic"),
            AddrComponent::WS => f.write_str("/ws"),
            AddrComponent::WSS => f.write_str("/wss"),
            AddrComponent::Libp2pWebsocketStar => f.write_str("/p2p-websocket-star"),
            AddrComponent::Libp2pWebrtcStar => f.write_str("/p2p-webrtc-star"),
            AddrComponent::Libp2pWebrtcDirect => f.write_str("/p2p-webrtc-direct"),
            AddrComponent::P2pCircuit => f.write_str("/p2p-circuit"),
            AddrComponent::Memory => f.write_str("/memory"),
        }
    }
}

impl<'a> From<Ipv4Addr> for AddrComponent<'a> {
    #[inline]
    fn from(addr: Ipv4Addr) -> Self {
        AddrComponent::IP4(addr)
    }
}

impl<'a> From<Ipv6Addr> for AddrComponent<'a> {
    #[inline]
    fn from(addr: Ipv6Addr) -> Self {
        AddrComponent::IP6(addr)
    }
}
