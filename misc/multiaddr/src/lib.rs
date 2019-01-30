///! # multiaddr
///!
///! Implementation of [multiaddr](https://github.com/jbenet/multiaddr)
///! in Rust.

#[macro_use]
extern crate arrayref;
extern crate bs58;
extern crate byteorder;
extern crate data_encoding;
extern crate serde;
extern crate unsigned_varint;
pub extern crate multihash;

mod protocol;
mod errors;

use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{self, Error as DeserializerError}
};
use std::{
    fmt,
    io,
    iter::FromIterator,
    net::{SocketAddr, SocketAddrV4, SocketAddrV6, IpAddr, Ipv4Addr, Ipv6Addr},
    result::Result as StdResult,
    str::FromStr
};
pub use self::errors::{Result, Error};
pub use self::protocol::Protocol;

/// Representation of a Multiaddr.
#[derive(PartialEq, Eq, Clone, Hash)]
pub struct Multiaddr { bytes: Vec<u8> }

impl Serialize for Multiaddr {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
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
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Multiaddr;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("multiaddress")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> StdResult<Self::Value, E> {
                v.parse().map_err(DeserializerError::custom)
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> StdResult<Self::Value, E> {
                self.visit_str(v)
            }
            fn visit_string<E: de::Error>(self, v: String) -> StdResult<Self::Value, E> {
                self.visit_str(&v)
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> StdResult<Self::Value, E> {
                self.visit_byte_buf(v.into())
            }
            fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> StdResult<Self::Value, E> {
                self.visit_byte_buf(v.into())
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> StdResult<Self::Value, E> {
                Multiaddr::from_bytes(v).map_err(DeserializerError::custom)
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_str(Visitor)
        } else {
            deserializer.deserialize_bytes(Visitor)
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
    /// use parity_multiaddr::Multiaddr;
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
    /// Create a new, empty multiaddress.
    pub fn empty() -> Multiaddr {
        Multiaddr { bytes: Vec::new() }
    }

    /// Returns the raw bytes representation of the multiaddr.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Return a copy to disallow changing the bytes directly
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// Produces a `Multiaddr` from its bytes representation.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Multiaddr> {
        {
            let mut ptr = &bytes[..];
            while !ptr.is_empty() {
                let (_, new_ptr) = Protocol::from_bytes(ptr)?;
                ptr = new_ptr;
            }
        }
        Ok(Multiaddr { bytes })
    }

    /// Extracts a slice containing the entire underlying vector.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Wrap a given Multiaddr and return the combination.
    ///
    /// # Examples
    ///
    /// ```
    /// use parity_multiaddr::Multiaddr;
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
        Ok(Multiaddr { bytes })
    }

    /// Adds an already-parsed address component to the end of this multiaddr.
    ///
    /// # Examples
    ///
    /// ```
    /// use parity_multiaddr::{Multiaddr, Protocol};
    ///
    /// let mut address: Multiaddr = "/ip4/127.0.0.1".parse().unwrap();
    /// address.append(Protocol::Tcp(10000));
    /// assert_eq!(address, "/ip4/127.0.0.1/tcp/10000".parse().unwrap());
    /// ```
    ///
    #[inline]
    pub fn append(&mut self, p: Protocol) {
        let n = self.bytes.len();
        let mut w = io::Cursor::new(&mut self.bytes);
        w.set_position(n as u64);
        p.write_bytes(&mut w).expect("writing to a Vec never fails")
    }

    /// Remove the outermost address.
    ///
    /// # Examples
    ///
    /// ```
    /// use parity_multiaddr::{Multiaddr, ToMultiaddr};
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
    /// use parity_multiaddr::ToMultiaddr;
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

            if self.bytes[i..next] == input[..] {
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

        Ok(Multiaddr { bytes })
    }

    /// Returns the components of this multiaddress.
    ///
    /// ```
    /// use std::net::Ipv4Addr;
    /// use parity_multiaddr::{Multiaddr, Protocol};
    ///
    /// let address: Multiaddr = "/ip4/127.0.0.1/udt/sctp/5678".parse().unwrap();
    ///
    /// let components = address.iter().collect::<Vec<_>>();
    /// assert_eq!(components[0], Protocol::Ip4(Ipv4Addr::new(127, 0, 0, 1)));
    /// assert_eq!(components[1], Protocol::Udt);
    /// assert_eq!(components[2], Protocol::Sctp(5678));
    /// ```
    ///
    #[inline]
    pub fn iter(&self) -> Iter {
        Iter(&self.bytes)
    }

    /// Pops the last `Protocol` of this multiaddr, or `None` if the multiaddr is empty.
    /// ```
    /// use parity_multiaddr::{Multiaddr, Protocol};
    ///
    /// let mut address: Multiaddr = "/ip4/127.0.0.1/udt/sctp/5678".parse().unwrap();
    ///
    /// assert_eq!(address.pop().unwrap(), Protocol::Sctp(5678));
    /// assert_eq!(address.pop().unwrap(), Protocol::Udt);
    /// ```
    ///
    pub fn pop<'a>(&mut self) -> Option<Protocol<'a>> {
        // Note: could be more optimized
        let mut list = self.iter().map(|p| p.acquire()).collect::<Vec<_>>();
        let last_elem = list.pop();
        *self = list.into_iter().collect();
        last_elem
    }
}

impl<'a> From<Protocol<'a>> for Multiaddr {
    fn from(p: Protocol<'a>) -> Multiaddr {
        let mut w = Vec::new();
        p.write_bytes(&mut w).expect("writing to a Vec never fails");
        Multiaddr { bytes: w }
    }
}

impl<'a> IntoIterator for &'a Multiaddr {
    type Item = Protocol<'a>;
    type IntoIter = Iter<'a>;

    #[inline]
    fn into_iter(self) -> Iter<'a> {
        Iter(&self.bytes)
    }
}

impl<'a> FromIterator<Protocol<'a>> for Multiaddr {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = Protocol<'a>>,
    {
        let mut writer = Vec::new();
        for cmp in iter {
            cmp.write_bytes(&mut writer).expect("writing to a Vec never fails");
        }
        Multiaddr { bytes: writer }
    }
}

impl FromStr for Multiaddr {
    type Err = Error;

    #[inline]
    fn from_str(input: &str) -> Result<Self> {
        let mut writer = Vec::new();
        let mut parts = input.split('/').peekable();

        if Some("") != parts.next() {
            // A multiaddr must start with `/`
            return Err(Error::InvalidMultiaddr)
        }

        while parts.peek().is_some() {
            let p = Protocol::from_str_parts(&mut parts)?;
            p.write_bytes(&mut writer).expect("writing to a Vec never fails");
        }

        Ok(Multiaddr { bytes: writer })
    }
}

/// Iterator for the address components in a multiaddr.
pub struct Iter<'a>(&'a [u8]);

impl<'a> Iterator for Iter<'a> {
    type Item = Protocol<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }

        let (p, next_data) =
            Protocol::from_bytes(self.0).expect("multiaddr is known to be valid");

        self.0 = next_data;
        Some(p)
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
        let mut m = self.ip().to_multiaddr()?;
        m.append(Protocol::Tcp(self.port()));
        Ok(m)
    }
}

impl ToMultiaddr for SocketAddrV6 {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        // TODO: Should we handle `flowinfo` and `scope_id`?
        let mut m = self.ip().to_multiaddr()?;
        m.append(Protocol::Tcp(self.port()));
        Ok(m)
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
        Ok(Protocol::Ip4(*self).into())
    }
}

impl ToMultiaddr for Ipv6Addr {
    fn to_multiaddr(&self) -> Result<Multiaddr> {
        Ok(Protocol::Ip6(*self).into())
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

