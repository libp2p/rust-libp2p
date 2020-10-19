use libp2p_core::identity::error::{DecodingError, SigningError as IdentitySigningError};
use libp2p_core::identity::Keypair;
use libp2p_core::keys_proto;
use libp2p_core::PublicKey;
use std::convert::{TryFrom, TryInto};
use std::fmt::Debug;
use unsigned_varint::encode;
use Error::*;

pub mod envelope_proto {
    include!(concat!(env!("OUT_DIR"), "/envelope_proto.rs"));
}

pub trait Record: TryFrom<Vec<u8>> + TryInto<Vec<u8>> + Clone {
    fn payload_type() -> &'static [u8];
    fn domain() -> &'static str;
}

pub struct Envelope<T: Record> {
    pub content: T,
    pub public_key: PublicKey,
    pub signature: Vec<u8>,

    payload: Vec<u8>,
}

#[derive(Debug)]
pub enum Error<T: Record>
where
    <T as TryInto<Vec<u8>>>::Error: Debug,
    <T as TryFrom<Vec<u8>>>::Error: Debug,
{
    SerializationError(<T as TryInto<Vec<u8>>>::Error),
    DeserializationError(<T as TryFrom<Vec<u8>>>::Error),
    EmptyDomain,
    EmptyPayload,
    SigningError(IdentitySigningError),
    NoPublicKey,
    InvalidSignature,
    PublicKeyDecodingError(DecodingError),
    WrongPayloadType,
}

impl<T: Record> Envelope<T>
where
    <T as TryInto<Vec<u8>>>::Error: Debug,
    <T as TryFrom<Vec<u8>>>::Error: Debug,
{
    pub fn sign(content: T, key_pair: &Keypair) -> Result<Self, Error<T>> {
        let payload: Vec<u8> = content.clone().try_into().map_err(SerializationError)?;

        let buffer = Self::get_buffer(&payload)?;

        let signature = key_pair.sign(&buffer).map_err(SigningError)?;

        Ok(Self {
            content,
            public_key: key_pair.public(),
            signature,
            payload,
        })
    }

    pub fn verify(&self) -> bool {
        if let Ok(buffer) = Self::get_buffer(&self.payload) {
            self.public_key.verify(&buffer, &self.signature)
        } else {
            false
        }
    }

    fn get_buffer(payload: &[u8]) -> Result<Vec<u8>, Error<T>> {
        if T::domain() == "" {
            return Err(EmptyDomain);
        }

        if payload.len() == 0 {
            return Err(EmptyPayload);
        }

        Ok(Self::concatenate_payloads(&[
            T::domain().as_bytes(),
            T::payload_type(),
            &payload,
        ]))
    }

    /// Concatenates all payloads and prefixes them with their length
    fn concatenate_payloads(payloads: &[&[u8]]) -> Vec<u8> {
        let mut result = Vec::new();
        let mut buf = encode::usize_buffer();
        for payload in payloads {
            result.extend_from_slice(encode::usize(payload.len(), &mut buf));
            result.extend_from_slice(payload);
        }
        result
    }
}

impl<T: Record> Into<envelope_proto::Envelope> for Envelope<T> {
    fn into(self) -> envelope_proto::Envelope {
        envelope_proto::Envelope {
            public_key: Some(self.public_key.into()),
            payload_type: T::payload_type().to_vec(),
            payload: self.payload,
            signature: self.signature,
        }
    }
}

impl<T: Record> TryFrom<envelope_proto::Envelope> for Envelope<T>
where
    <T as TryInto<Vec<u8>>>::Error: Debug,
    <T as TryFrom<Vec<u8>>>::Error: Debug,
{
    type Error = Error<T>;

    fn try_from(value: envelope_proto::Envelope) -> Result<Self, Self::Error> {
        let public_key = PublicKey::try_from(value.public_key.ok_or(NoPublicKey)?)
            .map_err(PublicKeyDecodingError)?;

        let payload = value.payload;

        if value.payload_type != T::payload_type() {
            return Err(WrongPayloadType);
        }

        let content = T::try_from(payload.clone()).map_err(DeserializationError)?;

        Ok(Self {
            content,
            public_key,
            signature: value.signature,
            payload,
        })
    }
}
