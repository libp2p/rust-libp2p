///! # multiaddr
///!
///! Implementation of [multiaddr](https://github.com/jbenet/multiaddr)
///! in Rust.

extern crate bs58;
extern crate byteorder;
extern crate serde;
extern crate unsigned_varint;
pub extern crate multihash;

mod protocol;
mod errors;

pub use errors::{Result, Error};
pub use protocol::{Protocol, ProtocolArgSize, AddrComponent};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializerError};

use std::fmt;
use std::result::Result as StdResult;
use std::iter::FromIterator;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6, IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

/// Representation of a Multiaddr.
#[derive(PartialEq, Eq, Clone, Hash)]
pub struct Multiaddr {
    bytes: Vec<u8>,
}

impl Serialize for Multiaddr {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            // Serialize to a human-readable string "2015-05-15T17:01:00Z".
            self.to_string().serialize(serializer)
        } else {
            self.to_bytes().serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Multiaddr {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let addr: String = Deserialize::deserialize(deserializer)?;
            addr.parse::<Multiaddr>().map_err(|err| DeserializerError::custom(err))
        } else {
            let addr: Vec<u8> = Deserialize::deserialize(deserializer)?;
            Multiaddr::from_bytes(addr).map_err(|err| DeserializerError::custom(err))
        }
    }
}

impl fmt::Debug for Multiaddr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.to_string().fmt(f)
    }
}

impl fmt::Display for Multiaddr {
    /// Convert a Multiaddr to a string
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::Multiaddr;
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1/udt".parse().unwrap();
    /// assert_eq!(address.to_string(), "/ip4/127.0.0.1/udt");
    /// ```
    ///
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for s in self.iter() {
            s.to_string().fmt(f)?;
        }
        Ok(())
    }
}

impl Multiaddr {
    /// Returns the raw bytes representation of the multiaddr.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Return a copy to disallow changing the bytes directly
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.to_owned()
    }

    /// Produces a `Multiaddr` from its bytes representation.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Multiaddr> {
        {
            let mut ptr = &bytes[..];
            while !ptr.is_empty() {
                let (_, new_ptr) = AddrComponent::from_bytes(ptr)?;
                ptr = new_ptr;
            }
        }

        Ok(Multiaddr { bytes })
    }

    /// Extracts a slice containing the entire underlying vector.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Return a list of protocols
    ///
    /// # Examples
    ///
    /// A single protocol
    ///
    /// ```
    /// use multiaddr::{Multiaddr, Protocol};
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1".parse().unwrap();
    /// assert_eq!(address.protocol(), vec![Protocol::IP4]);
    /// ```
    ///
    #[inline]
    #[deprecated(note = "Use `self.iter().map(|addr| addr.protocol_id())` instead")]
    pub fn protocol(&self) -> Vec<Protocol> {
        self.iter().map(|addr| addr.protocol_id()).collect()
    }

    /// Wrap a given Multiaddr and return the combination.
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::Multiaddr;
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1".parse().unwrap();
    /// let nested = address.encapsulate("/udt").unwrap();
    /// assert_eq!(nested, "/ip4/127.0.0.1/udt".parse().unwrap());
    /// ```
    ///
    pub fn encapsulate<T: ToMultiaddr>(&self, input: T) -> Result<Multiaddr> {
        let new = input.to_multiaddr()?;
        let mut bytes = self.bytes.clone();

        bytes.extend(new.to_bytes());

        Ok(Multiaddr { bytes: bytes })
    }

    /// Adds an already-parsed address component to the end of this multiaddr.
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::{Multiaddr, AddrComponent};
    ///
    /// let mut address: Multiaddr = "/ip4/127.0.0.1".parse().unwrap();
    /// address.append(AddrComponent::TCP(10000));
    /// assert_eq!(address, "/ip4/127.0.0.1/tcp/10000".parse().unwrap());
    /// ```
    ///
    #[inline]
    pub fn append(&mut self, component: AddrComponent) {
        component.write_bytes(&mut self.bytes).expect(
            "writing to a Vec never fails",
        )
    }

    /// Remove the outermost address.
    ///
    /// # Examples
    ///
    /// ```
    /// use multiaddr::{Multiaddr, ToMultiaddr};
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1/udt/sctp/5678".parse().unwrap();
    /// let unwrapped = address.decapsulate("/udt").unwrap();
    /// assert_eq!(unwrapped, "/ip4/127.0.0.1".parse().unwrap());
    ///
    /// assert_eq!(
    ///     address.decapsulate("/udt").unwrap(),
    ///     "/ip4/127.0.0.1".to_multiaddr().unwrap()
    /// );
    /// ```
    ///
    /// Returns the original if the passed in address is not found
    ///
    /// ```
    /// use multiaddr::ToMultiaddr;
    ///
    /// let address = "/ip4/127.0.0.1/udt/sctp/5678".to_multiaddr().unwrap();
    /// let unwrapped = address.decapsulate("/ip4/127.0.1.1").unwrap();
    /// assert_eq!(unwrapped, address);
    /// ```
    ///
    pub fn decapsulate<T: ToMultiaddr>(&self, input: T) -> Result<Multiaddr> {
        let input = input.to_multiaddr()?.to_bytes();

        let bytes_len = self.bytes.len();
        let input_length = input.len();

        let mut input_pos = 0;
        let mut matches = false;

        for (i, _) in self.bytes.iter().enumerate() {
            let next = i + input_length;

            if next > bytes_len {
                continue;
            }

            if &self.bytes[i..next] == input.as_slice() {
                matches = true;
                input_pos = i;
                break;
            }
        }

        if !matches {
            return Ok(Multiaddr { bytes: self.bytes.clone() });
        }

        let mut bytes = self.bytes.clone();
        bytes.truncate(input_pos);

        Ok(Multiaddr { bytes: bytes })
    }

    /// Returns the components of this multiaddress.
    ///
    /// ```
    /// use std::net::Ipv4Addr;
    /// use multiaddr::AddrComponent;
    /// use multiaddr::Multiaddr;
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1/udt/sctp/5678".parse().unwrap();
    ///
    /// let components = address.iter().collect::<Vec<_>>();
    /// assert_eq!(components[0], AddrComponent::IP4(Ipv4Addr::new(127, 0, 0, 1)));
    /// assert_eq!(components[1], AddrComponent::UDT);
    /// assert_eq!(components[2], AddrComponent::SCTP(5678));
    /// ```
    ///
    #[inline]
    pub fn iter(&self) -> Iter {
        Iter(&self.bytes)
    }

    /// Pops the last `AddrComponent` of this multiaddr, or `None` if the multiaddr is empty.
    /// ```
    /// use multiaddr::AddrComponent;
    /// use multiaddr::Multiaddr;
    ///
    /// let mut address: Multiaddr = "/ip4/127.0.0.1/udt/sctp/5678".parse().unwrap();
    ///
    /// assert_eq!(address.pop().unwrap(), AddrComponent::SCTP(5678));
    /// assert_eq!(address.pop().unwrap(), AddrComponent::UDT);
    /// ```
    ///
    pub fn pop<'a>(&mut self) -> Option<AddrComponent<'a>> {
        // Note: could be more optimized
        let mut list = self.iter().map(AddrComponent::acquire).collect::<Vec<_>>();
        let last_elem = list.pop();
        *self = list.into_iter().collect();
        last_elem
    }
}

impl<'a> From<AddrComponent<'a>> for Multiaddr {
    fn from(addr: AddrComponent) -> Multiaddr {
        let mut out = Vec::new();
        addr.write_bytes(&mut out).expect(
            "writing to a Vec never fails",
        );
        Multiaddr { bytes: out }
    }
}

impl<'a> IntoIterator for &'a Multiaddr {
    type Item = AddrComponent<'a>;
    type IntoIter = Iter<'a>;

    #[inline]
    fn into_iter(self) -> Iter<'a> {
        Iter(&self.bytes)
    }
}

impl<'a> FromIterator<AddrComponent<'a>> for Multiaddr {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = AddrComponent<'a>>,
    {
        let mut bytes = Vec::new();
        for cmp in iter {
            cmp.write_bytes(&mut bytes).expect(
                "writing to a Vec never fails",
            );
        }
        Multiaddr { bytes: bytes }
    }
}

impl FromStr for Multiaddr {
    type Err = Error;

    #[inline]
    fn from_str(input: &str) -> Result<Self> {
        let mut bytes = Vec::new();

        let mut parts = input.split('/');
        // A multiaddr must start with `/`
        if !parts.next().ok_or(Error::InvalidMultiaddr)?.is_empty() {
            return Err(Error::InvalidMultiaddr);
        }

        while let Some(part) = parts.next() {
            let protocol: Protocol = part.parse()?;
            let addr_component = match protocol.size() {
                ProtocolArgSize::Fixed { bytes: 0 } => {
                    protocol.parse_data("")? // TODO: bad design
                }
                _ => {
                    let data = parts.next().ok_or(Error::MissingAddress)?;
                    protocol.parse_data(data)?
                }
            };

            addr_component.write_bytes(&mut bytes).expect(
                "writing to a Vec never fails",
            );
        }

        Ok(Multiaddr { bytes: bytes })
    }
}

/// Iterator for the address components in a multiaddr.
pub struct Iter<'a>(&'a [u8]);

impl<'a> Iterator for Iter<'a> {
    type Item = AddrComponent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }

        let (component, next_data) =
            AddrComponent::from_bytes(self.0).expect("multiaddr is known to be valid");
        self.0 = next_data;
        Some(component)
    }
}

/// A trait for objects which can be converted to a
/// Multiaddr.
///
/// This trait is implemented by default for
///
/// * `SocketAddr`, `SocketAddrV4` and `SocketAddrV6`, assuming that the
///   the given port is a tcp port.
///
/// * `Ipv4Addr`, `Ipv6Addr`
///
/// * `String` and `&str`, requiring the default string format for a Multiaddr.
///
pub trait ToMultiaddr {
    /// Converts this object to a Multiaddr
    ///
    /// # Errors
    ///
    /// Any errors encountered during parsing will be returned
    /// as an `Err`.
    fn to_multiaddr(&self) -> Result<Multiaddr>;
}

impl ToMultiaddr for SocketAddr {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        match *self {
            SocketAddr::V4(ref a) => (*a).to_multiaddr(),
            SocketAddr::V6(ref a) => (*a).to_multiaddr(),
        }
    }
}

impl ToMultiaddr for SocketAddrV4 {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        format!("/ip4/{}/tcp/{}", self.ip(), self.port()).parse()
    }
}

impl ToMultiaddr for SocketAddrV6 {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        // TODO: Should how should we handle `flowinfo` and `scope_id`?
        format!("/ip6/{}/tcp/{}", self.ip(), self.port()).parse()
    }
}

impl ToMultiaddr for IpAddr {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        match *self {
            IpAddr::V4(ref a) => (*a).to_multiaddr(),
            IpAddr::V6(ref a) => (*a).to_multiaddr(),
        }
    }
}

impl ToMultiaddr for Ipv4Addr {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        format!("/ip4/{}", &self).parse()
    }
}

impl ToMultiaddr for Ipv6Addr {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        format!("/ip6/{}", &self).parse()
    }
}

impl ToMultiaddr for String {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        self.parse()
    }
}

impl<'a> ToMultiaddr for &'a str {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        self.parse()
    }
}

impl ToMultiaddr for Multiaddr {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        Ok(self.clone())
    }
}
