use std::borrow::Cow;
use std::fmt::Debug;
use serde::export::Formatter;
use arrayref::array_ref;
use std::fmt;

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
    pub fn into_owned<'b>(self) -> Onion3Addr<'b> {
        Self(Cow::Owned(self.0.into_owned()), self.1)
    }
}

impl PartialEq for Onion3Addr<'_> {
    fn eq(&self, other: &Self) -> bool {
        if self.1 != other.1 {
            return false
        }

        for i in 0..35 {
            if self.0[i] != other.0[i] {
                return false
            }
        }

        true
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

impl<'a> From<(&'a [u8], u16)> for Onion3Addr<'a> {
    fn from(parts: (&'a [u8], u16)) -> Self {
        Self(Cow::Borrowed(array_ref!(parts.0, 0, 35)), parts.1)
    }
}

impl From<(Vec<u8>, u16)> for Onion3Addr<'_> {
    fn from(parts: (Vec<u8>, u16)) -> Self {
        let slice = array_ref!(parts.0, 0, 35);
        Self(Cow::Owned(*slice), parts.1)
    }
}

impl Debug for Onion3Addr<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        let bytes_str = {
            let s = self.0[..].iter().map(ToString::to_string).collect::<Vec<_>>();
            format!("[{}]", s.join(", "))
        };
       f.debug_tuple("Onion3Addr")
           .field(&bytes_str)
           .field(&self.1)
           .finish()
    }
}
