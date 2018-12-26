// Not used, might be useful.

/// Represents the hash of an `RPC`.
///
/// Instead of a using the RPC as a whole, the API of gossipsub uses a
/// hash of the RPC. You only have to build the hash once, then use it
/// everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RPCHash {
    hash: String,
}

impl RPCHash {
    /// Builds a new `RPCHash` from the given hash.
    #[inline]
    pub fn from_raw(hash: String) -> RPCHash {
        RPCHash { hash: hash }
    }

    /// Converts an `RPCHash` into a hash of the message as a `String`.
    #[inline]
    pub fn into_string(self) -> String {
        self.hash
    }
}

/// Built RPC.
#[derive(Debug, Clone)]
pub struct RPC {
    descriptor: rpc_proto::RPC,
    hash: RPCHash,
}

impl RPC {
    /// Return the hash of the RPC.
    #[inline]
    pub fn hash(&self) -> &RPCHash {
        &self.hash
    }
}

impl AsRef<RPCHash> for RPC {
    #[inline]
    fn as_ref(&self) -> &RPCHash {
        &self.hash
    }
}

/// Builder for an `RPCHash`.
#[derive(Debug, Clone)]
pub struct RPCBuilder {
    builder: rpc_proto::RPC,
}

impl RPCBuilder {
    pub fn new<R>(rpc: R) -> RPCBuilder
    where
        R: Into<RPC>,
    {
        let mut builder = rpc_proto::RPC::new();

        RPCBuilder { builder: builder }
    }

    /// Turns the builder into an actual `RPC`.
    pub fn build(self) -> RPC {
        let bytes = self
            .builder
            .write_to_bytes()
            .expect("protobuf message is always valid");
        // TODO: https://github.com/libp2p/rust-libp2p/issues/473
        let hash = RPCHash {
            hash: bs58::encode(&bytes).into_string(),
        };
        RPC {
            descriptor: self.builder,
            hash,
        }
    }
}