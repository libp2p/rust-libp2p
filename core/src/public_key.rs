// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use PeerId;
use keys_proto;
use protobuf::{self, Message};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};

/// Public key used by the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicKey {
    /// DER format.
    Rsa(Vec<u8>),
    /// Format = ???
    // TODO: ^
    Ed25519(Vec<u8>),
    /// Format = ???
    // TODO: ^
    Secp256k1(Vec<u8>),
}

impl PublicKey {
    /// Encodes the public key as a protobuf message.
    ///
    /// Used at various locations in the wire protocol of libp2p.
    #[inline]
    pub fn into_protobuf_encoding(self) -> Vec<u8> {
        let mut public_key = keys_proto::PublicKey::new();
        match self {
            PublicKey::Rsa(data) => {
                public_key.set_Type(keys_proto::KeyType::RSA);
                public_key.set_Data(data);
            },
            PublicKey::Ed25519(data) => {
                public_key.set_Type(keys_proto::KeyType::Ed25519);
                public_key.set_Data(data);
            },
            PublicKey::Secp256k1(data) => {
                public_key.set_Type(keys_proto::KeyType::Secp256k1);
                public_key.set_Data(data);
            },
        };

        public_key
            .write_to_bytes()
            .expect("protobuf writing should always be valid")
    }

    /// Decodes the public key from a protobuf message.
    ///
    /// Used at various locations in the wire protocol of libp2p.
    #[inline]
    pub fn from_protobuf_encoding(bytes: &[u8]) -> Result<PublicKey, IoError> {
        let mut pubkey = protobuf::parse_from_bytes::<keys_proto::PublicKey>(bytes)
            .map_err(|err| {
                debug!("failed to parse public key's protobuf encoding");
                IoError::new(IoErrorKind::InvalidData, err)
            })?;

        Ok(match pubkey.get_Type() {
            keys_proto::KeyType::RSA => {
                PublicKey::Rsa(pubkey.take_Data())
            },
            keys_proto::KeyType::Ed25519 => {
                PublicKey::Ed25519(pubkey.take_Data())
            },
            keys_proto::KeyType::Secp256k1 => {
                PublicKey::Secp256k1(pubkey.take_Data())
            },
        })
    }

    /// Builds a `PeerId` corresponding to the public key of the node.
    #[inline]
    pub fn into_peer_id(self) -> PeerId {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use rand::random;
    use PublicKey;

    #[test]
    fn key_into_protobuf_then_back() {
        let key = PublicKey::Rsa((0 .. 2048).map(|_| -> u8 { random() }).collect());
        let second = PublicKey::from_protobuf_encoding(&key.clone().into_protobuf_encoding()).unwrap();
        assert_eq!(key, second);
    }
}
