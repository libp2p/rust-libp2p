use either::Either;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Debug, Clone, Eq)]
pub struct Protocol {
    inner: Either<&'static str, Arc<str>>,
}

impl Protocol {
    /// Construct a new protocol from a static string slice.
    ///
    /// # Panics
    ///
    /// This function panics if the protocol does not start with a forward slash: `/`.
    pub const fn from_static(s: &'static str) -> Self {
        match s.as_bytes() {
            [b'/', ..] => {}
            _ => panic!("Protocols should start with a /"),
        }

        Protocol {
            inner: Either::Left(s),
        }
    }

    /// Attempt to construct a protocol from an owned string.
    ///
    /// This function will fail if the protocol does not start with a forward slash: `/`.
    /// Where possible, you should use [`Protocol::from_static`] instead to avoid allocations.
    pub fn try_from_owned(protocol: String) -> Result<Self, InvalidProtocol> {
        if !protocol.starts_with('/') {
            return Err(InvalidProtocol {});
        }

        Ok(Protocol {
            inner: Either::Right(Arc::from(protocol)), // FIXME: Can we somehow reuse the allocation from the owned string?
        })
    }
}

impl AsRef<str> for Protocol {
    fn as_ref(&self) -> &str {
        either::for_both!(&self.inner, s => s)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl PartialEq<&str> for Protocol {
    fn eq(&self, other: &&str) -> bool {
        self.as_ref() == *other
    }
}

impl PartialEq<Protocol> for &str {
    fn eq(&self, other: &Protocol) -> bool {
        *self == other.as_ref()
    }
}

impl PartialEq for Protocol {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl Hash for Protocol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state)
    }
}

#[derive(Debug)]
pub struct InvalidProtocol {}

impl fmt::Display for InvalidProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid protocol: string does not start with a forward slash"
        )
    }
}

impl std::error::Error for InvalidProtocol {}
