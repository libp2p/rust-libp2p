// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::kbucket::KBucketsPeerId;
use crate::parity::Namespace;
use lazy_static::lazy_static;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_core::upgrade::{self, InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use serde_derive::{Serialize, Deserialize};
use std::{cmp::PartialEq, error, fmt, iter};
use tokio_io::{AsyncRead, AsyncWrite};

/// Name of the protocol on the network.
const PROTOCOL_NAME: &[u8] = b"/paritytech/kad/1.0.0";
/// Maximum length of a message.
const MAX_MESSAGE_LEN: usize = 2048;

/// Request that can be sent to a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KadRequest {
    /// Report what our namespace and our listening multiaddresses. Should be sent first things
    /// first whenever we connect to a node.
    #[serde(rename = "NAMESPACE")]
    NamespaceReport([u8; 4]),
    /// Add to the DHT a peer that can sign with the given public key.
    #[serde(rename = "ADD_PROVIDER")]
    AddProvider([u8; 32], UnverifiedKadRecord),
    /// Find the peers that can sign with the given public key.
    #[serde(rename = "GET_PROVIDERS")]
    FindProviders([u8; 32]),
    /// Find the peer with the given identity.
    #[serde(rename = "FIND_PEER")]
    FindPeer(KadPeerId),
}

/// Response that can be received from a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KadResponse {
    /// Response to a `KadRequest::FindProviders`.
    #[serde(rename = "GET_PROVIDERS")]
    FindProviders {
        /// List of known peers that are the closest to the requested key.
        closer_peers: Vec<KadPeer>,
        /// List of records that have been found on the way. May be wrong and thus need to be
        /// verified.
        records: Vec<UnverifiedKadRecord>,
    },
    /// Response to a `KadRequest::FindPeer`.
    #[serde(rename = "FIND_PEER")]
    FindPeer {
        /// List of known peers that are the closest to the requested ID.
        closer_peers: Vec<KadPeer>,
    },
}

/// Identify of a peer in the DHT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KadPeerId {
    /// Namespace of the peer.
    pub namespace: [u8; 4],
    /// Network key of the peer.
    pub peer_id: PeerId,
}

impl KBucketsPeerId for KadPeerId {
    fn distance_with(&self, other: &KadPeerId) -> u32 {
        (&Namespace(self.namespace), &self.peer_id)
            .distance_with(&(&Namespace(other.namespace), &other.peer_id))
    }

    fn max_distance() -> usize {
        <(Namespace, PeerId) as KBucketsPeerId>::max_distance()
    }
}

impl KBucketsPeerId<(Namespace, PeerId)> for KadPeerId {
    fn distance_with(&self, other: &(Namespace, PeerId)) -> u32 {
        (&Namespace(self.namespace), &self.peer_id).distance_with(&(&other.0, &other.1))
    }

    fn max_distance() -> usize {
        <(Namespace, PeerId) as KBucketsPeerId>::max_distance()
    }
}

impl KBucketsPeerId<[u8; 32]> for KadPeerId {
    fn distance_with(&self, other: &[u8; 32]) -> u32 {
        unimplemented!()
    }

    fn max_distance() -> usize {
        <(Namespace, PeerId) as KBucketsPeerId>::max_distance()
    }
}

impl PartialEq<(Namespace, PeerId)> for KadPeerId {
    fn eq(&self, other: &(Namespace, PeerId)) -> bool {
        self.namespace == (other.0).0 && self.peer_id == other.1
    }
}

impl PartialEq<[u8; 32]> for KadPeerId {
    fn eq(&self, other: &[u8; 32]) -> bool {
        unimplemented!()        // TODO:
    }
}

impl serde::Serialize for KadPeerId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // TODO: inefficient
        let mut data = self.namespace.to_vec();
        data.extend(self.peer_id.as_bytes().iter().cloned());
        data.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for KadPeerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // TODO: inefficient
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        // TODO: check length
        let namespace = [bytes[0], bytes[1], bytes[2], bytes[3]];
        let peer_id = PeerId::from_bytes(bytes[4..].to_vec()).unwrap(); // TODO: no
        Ok(KadPeerId {
            namespace,
            peer_id,
        })
    }
}

/// Record for a value in Kademlia.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnverifiedKadRecord {
    /// Identity of the provider.
    pub identity: KadPeer,

    /// Unique identifier for the record. Each new record should have a counter greater than a
    /// previous one. Records with a higher counter erase records with a lower value.
    pub counter: u64,

    /// Signature of the raw CBOR representation of `KadPeer` combined with the counter, that can
    /// be verified against the public key this record corresponds to.
    pub signature: KadSignature,
}

impl UnverifiedKadRecord {
    /// Verifies that the `KadPeer`, the signature, and the public key match. If so, returns
    /// a `KadSignature`.
    pub fn verify(self, pubkey: &schnorrkel::PublicKey) -> Result<KadRecord, ()> {
        let bytes = serde_cbor::to_vec(&(&self.identity, self.counter))
            .expect("serde_cbor never fails to serialize; QED");
        let signature = schnorrkel::Signature::from_bytes(&self.signature.0).map_err(|_| ())?;
        if pubkey.verify(SIGN_CONTEXT.bytes(&bytes), &signature) {
            Ok(KadRecord {
                identity: self.identity,
                counter: self.counter,
                signature: self.signature,
            })
        } else {
            Err(())
        }
    }
}

/// Record for a value in Kademlia.
#[derive(Debug, Clone)]
pub struct KadRecord {
    /// Identity of the provider.
    identity: KadPeer,

    /// Unique identifier for the record. Each new record should have a counter greater than a
    /// previous one. Records with a higher counter erase records with a lower value.
    counter: u64,

    /// Signature of the raw CBOR representation of `KadPeer` that can be verified against the
    /// public key this record corresponds to.
    signature: KadSignature,
}

impl KadRecord {
    /// Signs a `KadPeer` and builds a `KadRecord` from it.
    pub fn from_kad_peer(kad_peer: KadPeer, counter: u64, key_pair: &schnorrkel::Keypair) -> KadRecord {
        let signature = kad_peer.sign(counter, key_pair);
        KadRecord {
            identity: kad_peer,
            counter,
            signature,
        }
    }

    /// Returns true if this record should replace `other` in a list of records.
    pub fn should_replace(&self, other: &KadRecord) -> bool {
        self.identity.id == other.identity.id && self.counter > other.counter
    }

    /// Returns the `KadPeer` inside the record.
    pub fn identity(&self) -> &KadPeer {
        &self.identity
    }

    /// Returns the `KadPeer` inside the record.
    pub fn into_identity(self) -> KadPeer {
        self.identity
    }

    /// Returns the signature of the record.
    pub fn signature(&self) -> &KadSignature {
        &self.signature
    }

    /// Returns the inner components.
    pub fn into_inner(self) -> (KadPeer, KadSignature) {
        (self.identity, self.signature)
    }
}

impl From<KadRecord> for UnverifiedKadRecord {
    fn from(record: KadRecord) -> UnverifiedKadRecord {
        UnverifiedKadRecord {
            identity: record.identity,
            counter: record.counter,
            signature: record.signature,
        }
    }
}

/// Schnorr-Ristretto signature of a `KadPeer`.
// TODO: hard-code format?
#[derive(Copy, Clone)]
pub struct KadSignature(pub [u8; 64]);

impl fmt::Debug for KadSignature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.0.iter()).finish()
    }
}

impl serde::Serialize for KadSignature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.to_vec().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for KadSignature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        assert_eq!(bytes.len(), 64);        // TODO: no, return error instead
        let mut out = [0; 64];
        out.copy_from_slice(&bytes);
        Ok(KadSignature(out))
    }
}

/// Information about a peer, transmitted on the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KadPeer {
    /// Identifier of the peer.
    pub id: KadPeerId,

    /// Addresses that the node listens on. Should be ordered by decreased likelyhood of
    /// reachability.
    pub addresses: Vec<Multiaddr>,
}

impl KadPeer {
    /// Produces a cryptographic signature of this `KadPeer`. The value of `counter` is included in
    /// the signature.
    pub fn sign(&self, counter: u64, keypair: &schnorrkel::Keypair) -> KadSignature {
        let my_bytes = serde_cbor::to_vec(&(self, counter))
            .expect("serde_cbor never fails to serialize; QED");
        let sig = keypair.sign(SIGN_CONTEXT.bytes(&my_bytes));
        KadSignature(sig.to_bytes())
    }
}

lazy_static! {
    /// Context for schnorr-ristretto signing.
    static ref SIGN_CONTEXT: schnorrkel::context::SigningContext =
        schnorrkel::signing_context(b"libp2p kademlia DHT records signing");
}

impl UpgradeInfo for KadRequest {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for KadRequest
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = (KadRequest, Option<KadResponse>);
    type Error = KadRequestError;
    type Future = upgrade::RequestResponse<TSocket, Self, fn(Vec<u8>, Self) -> Result<Self::Output, Self::Error>>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let message = serde_cbor::to_vec(&self)
            .expect("serde_cbor can never fail to encode a value; QED");
        upgrade::request_response(socket, message, MAX_MESSAGE_LEN, self, |response, sent| {
            let response = if response.is_empty() {
                None
            } else {
                let msg = serde_cbor::from_slice(&response)?;
                Some(msg)
            };
            Ok((sent, response))
        })
    }
}

/// Error while sending a push request.
#[derive(Debug)]
pub enum KadRequestError {
    /// Error while reading the message.
    Read(upgrade::ReadOneError),
    /// Failed to decode the message.
    Decode(serde_cbor::error::Error),
}

impl From<upgrade::ReadOneError> for KadRequestError {
    fn from(err: upgrade::ReadOneError) -> Self {
        KadRequestError::Read(err)
    }
}

impl From<serde_cbor::error::Error> for KadRequestError {
    fn from(err: serde_cbor::error::Error) -> Self {
        KadRequestError::Decode(err)
    }
}

impl fmt::Display for KadRequestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            KadRequestError::Read(ref err) => write!(f, "{}", err),
            KadRequestError::Decode(ref err) => write!(f, "{}", err),
        }
    }
}

impl error::Error for KadRequestError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            KadRequestError::Read(ref err) => Some(err),
            KadRequestError::Decode(ref err) => Some(err),
        }
    }
}

/// Upgrade that listens for a request from the remote, and allows answering it.
#[derive(Debug, Default, Clone)]
pub struct KadListen {
}

impl UpgradeInfo for KadListen {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<TSocket> InboundUpgrade<TSocket> for KadListen
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = KadListenOut<TSocket>;
    type Error = Box<error::Error + Send + Sync>;   // TODO: better error
    type Future = upgrade::ReadRespond<TSocket, Self, fn(TSocket, Vec<u8>, Self) -> Result<Self::Output, Self::Error>>;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        upgrade::read_respond(socket, MAX_MESSAGE_LEN, self, |socket, message_bytes, _me| {
            let request = serde_cbor::from_slice(&message_bytes)?;
            Ok(KadListenOut {
                request,
                socket,
            })
        })
    }
}

/// Request received from a remote.
#[derive(Debug)]
pub struct KadListenOut<TSocket> {
    /// The request we received.
    request: KadRequest,
    /// Socket where to respond.
    socket: TSocket,
}

impl<TSocket> KadListenOut<TSocket>
where
    TSocket: AsyncWrite,
{
    /// Returns the request received from the remote.
    pub fn request(&self) -> &KadRequest {
        &self.request
    }

    /// Responds to the message. Returns a future that sends the message and shuts down the
    /// substream.
    #[must_use]
    pub fn respond(self, message: KadResponse) -> upgrade::WriteOne<TSocket> {
        let message = serde_cbor::to_vec(&message)
            .expect("serde_cbor never fails to encode a message; QED");
        upgrade::write_one(self.socket, message)
    }
}

#[cfg(test)]
mod tests {
    use libp2p_core::multiaddr::Protocol;
    use super::*;

    #[test]
    fn sign_and_verify() {
        for _ in 0..200 {
            let keypair = schnorrkel::Keypair::generate(&mut rand::thread_rng());

            let identity = KadPeer {
                id: KadPeerId {
                    namespace: rand::random(),
                    peer_id: PeerId::random(),
                },
                addresses: (0..rand::random::<usize>() % 5).map(|_| {
                    std::iter::once(Protocol::Tcp(rand::random())).collect()
                }).collect()
            };

            let counter = rand::random();

            let record_bad_signature = UnverifiedKadRecord {
                identity: identity.clone(),
                counter,
                signature: {
                    let mut s = [0; 64];
                    for b in s.iter_mut() { *b = rand::random(); }
                    KadSignature(s)
                }
            };

            let record_bad_counter = UnverifiedKadRecord {
                identity: identity.clone(),
                counter: rand::random(),
                signature: identity.sign(counter, &keypair),
            };

            let good_record = UnverifiedKadRecord {
                identity: identity.clone(),
                counter,
                signature: identity.sign(counter, &keypair),
            };

            assert!(record_bad_signature.verify(&keypair.public).is_err());
            assert!(record_bad_counter.verify(&keypair.public).is_err());
            assert!(good_record.verify(&keypair.public).is_ok());
        }
    }
}
