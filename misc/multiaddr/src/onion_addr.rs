use std::{borrow::Cow, fmt};

/// Represents an Onion v3 address
#[derive(Clone)]
pub struct Onion3Addr<'a>(Cow<'a, [u8; 35]>, u16);

impl<'a> Onion3Addr<'a> {
    /// Return the hash of the public key as bytes
    pub fn hash(&self) -> &[u8; 35] {
        self.0.as_ref()
    }

    /// Return the port
    pub fn port(&self) -> u16 {
        self.1
    }

    /// Consume this instance and create an owned version containing the same address
    pub fn acquire<'b>(self) -> Onion3Addr<'b> {
        Onion3Addr(Cow::Owned(self.0.into_owned()), self.1)
    }
}

impl PartialEq for Onion3Addr<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.1 == other.1 && self.0[..] == other.0[..]
    }
}

impl Eq for Onion3Addr<'_> { }

impl From<([u8; 35], u16)> for Onion3Addr<'_> {
    fn from(parts: ([u8; 35], u16)) -> Self {
        Self(Cow::Owned(parts.0), parts.1)
    }
}

impl<'a> From<(&'a [u8; 35], u16)> for Onion3Addr<'a> {
    fn from(parts: (&'a [u8; 35], u16)) -> Self {
        Self(Cow::Borrowed(parts.0), parts.1)
    }
}

impl fmt::Debug for Onion3Addr<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
       f.debug_tuple("Onion3Addr")
           .field(&format!("{:02x?}", &self.0[..]))
           .field(&self.1)
           .finish()
    }
}
