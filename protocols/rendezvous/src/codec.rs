// Copyright 2021 COMIT Network.
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

use crate::DEFAULT_TTL;
use asynchronous_codec::{Bytes, BytesMut, Decoder, Encoder};
use libp2p_core::{peer_record, signed_envelope, PeerRecord, SignedEnvelope};
use rand::RngCore;
use std::convert::{TryFrom, TryInto};
use std::fmt;
use unsigned_varint::codec::UviBytes;

pub type Ttl = u64;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Message {
    Register(NewRegistration),
    RegisterResponse(Result<Ttl, ErrorCode>),
    Unregister(Namespace),
    Discover {
        namespace: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<Ttl>,
    },
    DiscoverResponse(Result<(Vec<Registration>, Cookie), ErrorCode>),
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Namespace(String);

impl Namespace {
    /// Creates a new [`Namespace`] from a static string.
    ///
    /// This will panic if the namespace is too long. We accepting panicking in this case because we are enforcing a `static lifetime which means this value can only be a constant in the program and hence we hope the developer checked that it is of an acceptable length.
    pub fn from_static(value: &'static str) -> Self {
        if value.len() > 255 {
            panic!("Namespace '{}' is too long!", value)
        }

        Namespace(value.to_owned())
    }

    pub fn new(value: String) -> Result<Self, NamespaceTooLong> {
        if value.len() > 255 {
            return Err(NamespaceTooLong);
        }

        Ok(Namespace(value))
    }
}

impl From<Namespace> for String {
    fn from(namespace: Namespace) -> Self {
        namespace.0
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<str> for Namespace {
    fn eq(&self, other: &str) -> bool {
        self.0.eq(other)
    }
}

impl PartialEq<Namespace> for str {
    fn eq(&self, other: &Namespace) -> bool {
        other.0.eq(self)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Namespace is too long")]
pub struct NamespaceTooLong;

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct Cookie {
    id: u64,
    namespace: Option<Namespace>,
}

impl Cookie {
    /// Construct a new [`Cookie`] for a given namespace.
    ///
    /// This cookie will only be valid for subsequent DISCOVER requests targeting the same namespace.
    pub fn for_namespace(namespace: Namespace) -> Self {
        Self {
            id: rand::thread_rng().next_u64(),
            namespace: Some(namespace),
        }
    }

    /// Construct a new [`Cookie`] for a DISCOVER request that inquires about all namespaces.
    pub fn for_all_namespaces() -> Self {
        Self {
            id: rand::random(),
            namespace: None,
        }
    }

    pub fn into_wire_encoding(self) -> Vec<u8> {
        let id_bytes = self.id.to_be_bytes();
        let namespace = self.namespace.map(|ns| ns.0).unwrap_or_default();

        let mut buffer = Vec::with_capacity(id_bytes.len() + namespace.len());
        buffer.extend_from_slice(&id_bytes);
        buffer.extend_from_slice(namespace.as_bytes());

        buffer
    }

    pub fn from_wire_encoding(mut bytes: Vec<u8>) -> Result<Self, InvalidCookie> {
        // check length early to avoid panic during slicing
        if bytes.len() < 8 {
            return Err(InvalidCookie);
        }

        let namespace = bytes.split_off(8);
        let namespace = if namespace.is_empty() {
            None
        } else {
            Some(
                Namespace::new(String::from_utf8(namespace).map_err(|_| InvalidCookie)?)
                    .map_err(|_| InvalidCookie)?,
            )
        };

        let bytes = <[u8; 8]>::try_from(bytes).map_err(|_| InvalidCookie)?;
        let id = u64::from_be_bytes(bytes);

        Ok(Self { id, namespace })
    }

    pub fn namespace(&self) -> Option<&Namespace> {
        self.namespace.as_ref()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("The cookie was malformed")]
pub struct InvalidCookie;

#[derive(Debug, Clone)]
pub struct NewRegistration {
    pub namespace: Namespace,
    pub record: PeerRecord,
    pub ttl: Option<u64>,
}

impl NewRegistration {
    pub fn new(namespace: Namespace, record: PeerRecord, ttl: Option<Ttl>) -> Self {
        Self {
            namespace,
            record,
            ttl,
        }
    }

    pub fn effective_ttl(&self) -> Ttl {
        self.ttl.unwrap_or(DEFAULT_TTL)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub namespace: Namespace,
    pub record: PeerRecord,
    pub ttl: Ttl,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidNamespace,
    InvalidSignedPeerRecord,
    InvalidTtl,
    InvalidCookie,
    NotAuthorized,
    InternalError,
    Unavailable,
}

pub struct RendezvousCodec {
    /// Codec to encode/decode the Unsigned varint length prefix of the frames.
    length_codec: UviBytes,
}

impl Default for RendezvousCodec {
    fn default() -> Self {
        let mut length_codec = UviBytes::default();
        length_codec.set_max_len(1024 * 1024); // 1MB

        Self { length_codec }
    }
}

impl Encoder for RendezvousCodec {
    type Item = Message;
    type Error = Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        use protobuf::Message;

        let message = wire::Message::from(item);

        let bytes = message
            .write_to_bytes()
            .expect("All required fields to be initialized.");

        // Length prefix the protobuf message, ensuring the max limit is not hit
        self.length_codec.encode(Bytes::from(bytes), dst)?;

        Ok(())
    }
}

impl Decoder for RendezvousCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        use protobuf::Message;

        let message = match self.length_codec.decode(src)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let message = wire::Message::parse_from_bytes(message.as_ref()).map_err(Error::Decode)?;

        Ok(Some(message.try_into()?))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to encode message as bytes")]
    Encode(protobuf::Error),
    #[error("Failed to decode message from bytes")]
    Decode(protobuf::Error),
    #[error("Failed to read/write")]
    Io(#[from] std::io::Error),
    #[error("Failed to convert wire message to internal data model")]
    Conversion(#[from] ConversionError),
}

impl From<Message> for wire::Message {
    fn from(message: Message) -> Self {
        use wire::message::*;

        match message {
            Message::Register(NewRegistration {
                namespace,
                record,
                ttl,
            }) => {
                let mut msg = wire::Message::new();
                msg.set_type(MessageType::REGISTER);
                msg.register = protobuf::MessageField::some(Register {
                    ns: Some(namespace.into()),
                    ttl,
                    signedPeerRecord: Some(record.into_signed_envelope().into_protobuf_encoding()),
                    ..Register::default()
                });
                msg
            }
            Message::RegisterResponse(Ok(ttl)) => {
                let mut msg = wire::Message::new();
                msg.set_type(MessageType::REGISTER_RESPONSE);
                msg.registerResponse = protobuf::MessageField::some({
                    let mut res = RegisterResponse::new();
                    res.set_status(ResponseStatus::OK);
                    res.set_ttl(ttl);
                    res
                });
                msg
            }
            Message::RegisterResponse(Err(error)) => {
                let mut msg = wire::Message::new();
                msg.set_type(MessageType::REGISTER_RESPONSE);
                msg.registerResponse = protobuf::MessageField::some({
                    let mut res = RegisterResponse::new();
                    res.set_status(ResponseStatus::from(error));
                    res
                });
                msg
            }
            Message::Unregister(namespace) => {
                let mut msg = wire::Message::new();
                msg.set_type(MessageType::UNREGISTER);
                msg.unregister = protobuf::MessageField::some(Unregister {
                    ns: Some(namespace.into()),
                    id: None,
                    ..Unregister::default()
                });
                msg
            }
            Message::Discover {
                namespace,
                cookie,
                limit,
            } => {
                let mut msg = wire::Message::new();
                msg.set_type(MessageType::DISCOVER);
                msg.discover = protobuf::MessageField::some(Discover {
                    ns: namespace.map(|ns| ns.into()),
                    cookie: cookie.map(|cookie| cookie.into_wire_encoding()),
                    limit,
                    ..Discover::default()
                });
                msg
            }
            Message::DiscoverResponse(Ok((registrations, cookie))) => {
                let mut msg = wire::Message::new();
                msg.set_type(MessageType::DISCOVER_RESPONSE);
                msg.discoverResponse = protobuf::MessageField::some(DiscoverResponse {
                    registrations: registrations
                        .into_iter()
                        .map(|reggo| Register {
                            ns: Some(reggo.namespace.into()),
                            ttl: Some(reggo.ttl),
                            signedPeerRecord: Some(
                                reggo.record.into_signed_envelope().into_protobuf_encoding(),
                            ),
                            ..Register::default()
                        })
                        .collect(),
                    status: Some(ResponseStatus::OK.into()),
                    cookie: Some(cookie.into_wire_encoding()),
                    ..DiscoverResponse::default()
                });
                msg
            }
            Message::DiscoverResponse(Err(error)) => {
                let mut msg = wire::Message::new();
                msg.set_type(MessageType::DISCOVER_RESPONSE);
                msg.discoverResponse = protobuf::MessageField::some(DiscoverResponse {
                    status: Some(ResponseStatus::from(error).into()),
                    ..DiscoverResponse::default()
                });
                msg
            }
        }
    }
}

impl TryFrom<wire::Message> for Message {
    type Error = ConversionError;

    fn try_from(message: wire::Message) -> Result<Self, Self::Error> {
        use wire::message::*;

        let msg_ty = message
            .type_
            .ok_or(ConversionError::InconsistentWireMessage)?
            .enum_value()
            .or(Err(ConversionError::InconsistentWireMessage))?;

        let message = match msg_ty {
            MessageType::REGISTER => {
                let register = message
                    .register
                    .into_option()
                    .ok_or(ConversionError::InconsistentWireMessage)?;
                let signed_peer_record = register
                    .signedPeerRecord
                    .ok_or(ConversionError::InconsistentWireMessage)?;
                Message::Register(NewRegistration {
                    namespace: register
                        .ns
                        .map(Namespace::new)
                        .transpose()?
                        .ok_or(ConversionError::MissingNamespace)?,
                    ttl: register.ttl,
                    record: PeerRecord::from_signed_envelope(
                        SignedEnvelope::from_protobuf_encoding(&signed_peer_record)?,
                    )?,
                })
            }
            MessageType::REGISTER_RESPONSE => {
                let register_response = message
                    .registerResponse
                    .into_option()
                    .ok_or(ConversionError::InconsistentWireMessage)?;
                let status = register_response
                    .status
                    .ok_or(ConversionError::InconsistentWireMessage)?
                    .enum_value()
                    .or(Err(ConversionError::BadStatusCode))?;

                match status {
                    ResponseStatus::OK => Message::RegisterResponse(Ok(register_response
                        .ttl
                        .ok_or(ConversionError::MissingTtl)?)),
                    error_code => Message::RegisterResponse(Err(error_code.try_into()?)),
                }
            }
            MessageType::UNREGISTER => {
                let unregister = message
                    .unregister
                    .into_option()
                    .ok_or(ConversionError::InconsistentWireMessage)?;

                Message::Unregister(
                    unregister
                        .ns
                        .map(Namespace::new)
                        .transpose()?
                        .ok_or(ConversionError::MissingNamespace)?,
                )
            }
            MessageType::DISCOVER => {
                let discover = message
                    .discover
                    .into_option()
                    .ok_or(ConversionError::InconsistentWireMessage)?;

                Message::Discover {
                    namespace: discover.ns.map(Namespace::new).transpose()?,
                    cookie: discover
                        .cookie
                        .map(Cookie::from_wire_encoding)
                        .transpose()?,
                    limit: discover.limit,
                }
            }
            MessageType::DISCOVER_RESPONSE => {
                let discover_response = message
                    .discoverResponse
                    .into_option()
                    .ok_or(ConversionError::InconsistentWireMessage)?;
                let status = discover_response
                    .status
                    .ok_or(ConversionError::InconsistentWireMessage)?
                    .enum_value()
                    .or(Err(ConversionError::BadStatusCode))?;

                match status {
                    ResponseStatus::OK => {
                        let registrations = discover_response
                            .registrations
                            .into_iter()
                            .map(|reggo| {
                                Ok(Registration {
                                    namespace: reggo
                                        .ns
                                        .map(Namespace::new)
                                        .transpose()?
                                        .ok_or(ConversionError::MissingNamespace)?,
                                    record: PeerRecord::from_signed_envelope(
                                        SignedEnvelope::from_protobuf_encoding(
                                            &reggo
                                                .signedPeerRecord
                                                .ok_or(ConversionError::MissingSignedPeerRecord)?,
                                        )?,
                                    )?,
                                    ttl: reggo.ttl.ok_or(ConversionError::MissingTtl)?,
                                })
                            })
                            .collect::<Result<Vec<_>, ConversionError>>()?;
                        let cookie = Cookie::from_wire_encoding(
                            discover_response
                                .cookie
                                .ok_or(ConversionError::InconsistentWireMessage)?,
                        )?;

                        Message::DiscoverResponse(Ok((registrations, cookie)))
                    }
                    error_code => Message::DiscoverResponse(Err(error_code.try_into()?)),
                }
            }
        };

        Ok(message)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    #[error("The wire message is consistent")]
    InconsistentWireMessage,
    #[error("Missing namespace field")]
    MissingNamespace,
    #[error("Invalid namespace")]
    InvalidNamespace(#[from] NamespaceTooLong),
    #[error("Missing signed peer record field")]
    MissingSignedPeerRecord,
    #[error("Missing TTL field")]
    MissingTtl,
    #[error("Bad status code")]
    BadStatusCode,
    #[error("Failed to decode signed envelope")]
    BadSignedEnvelope(#[from] signed_envelope::DecodingError),
    #[error("Failed to decode envelope as signed peer record")]
    BadSignedPeerRecord(#[from] peer_record::FromEnvelopeError),
    #[error(transparent)]
    BadCookie(#[from] InvalidCookie),
    #[error("The requested PoW difficulty is out of range")]
    PoWDifficultyOutOfRange,
    #[error("The provided PoW hash is not 32 bytes long")]
    BadPoWHash,
}

impl ConversionError {
    pub fn to_error_code(&self) -> ErrorCode {
        match self {
            ConversionError::MissingNamespace => ErrorCode::InvalidNamespace,
            ConversionError::MissingSignedPeerRecord => ErrorCode::InvalidSignedPeerRecord,
            ConversionError::BadSignedEnvelope(_) => ErrorCode::InvalidSignedPeerRecord,
            ConversionError::BadSignedPeerRecord(_) => ErrorCode::InvalidSignedPeerRecord,
            ConversionError::BadCookie(_) => ErrorCode::InvalidCookie,
            ConversionError::MissingTtl => ErrorCode::InvalidTtl,
            ConversionError::InconsistentWireMessage => ErrorCode::InternalError,
            ConversionError::BadStatusCode => ErrorCode::InternalError,
            ConversionError::PoWDifficultyOutOfRange => ErrorCode::InternalError,
            ConversionError::BadPoWHash => ErrorCode::InternalError,
            ConversionError::InvalidNamespace(_) => ErrorCode::InvalidNamespace,
        }
    }
}

impl TryFrom<wire::message::ResponseStatus> for ErrorCode {
    type Error = UnmappableStatusCode;

    fn try_from(value: wire::message::ResponseStatus) -> Result<Self, Self::Error> {
        use wire::message::ResponseStatus::*;

        let code = match value {
            OK => return Err(UnmappableStatusCode(value)),
            E_INVALID_NAMESPACE => ErrorCode::InvalidNamespace,
            E_INVALID_SIGNED_PEER_RECORD => ErrorCode::InvalidSignedPeerRecord,
            E_INVALID_TTL => ErrorCode::InvalidTtl,
            E_INVALID_COOKIE => ErrorCode::InvalidCookie,
            E_NOT_AUTHORIZED => ErrorCode::NotAuthorized,
            E_INTERNAL_ERROR => ErrorCode::InternalError,
            E_UNAVAILABLE => ErrorCode::Unavailable,
        };

        Result::Ok(code)
    }
}

impl From<ErrorCode> for wire::message::ResponseStatus {
    fn from(error_code: ErrorCode) -> Self {
        use wire::message::ResponseStatus::*;

        match error_code {
            ErrorCode::InvalidNamespace => E_INVALID_NAMESPACE,
            ErrorCode::InvalidSignedPeerRecord => E_INVALID_SIGNED_PEER_RECORD,
            ErrorCode::InvalidTtl => E_INVALID_TTL,
            ErrorCode::InvalidCookie => E_INVALID_COOKIE,
            ErrorCode::NotAuthorized => E_NOT_AUTHORIZED,
            ErrorCode::InternalError => E_INTERNAL_ERROR,
            ErrorCode::Unavailable => E_UNAVAILABLE,
        }
    }
}

impl From<UnmappableStatusCode> for ConversionError {
    fn from(_: UnmappableStatusCode) -> Self {
        ConversionError::InconsistentWireMessage
    }
}

#[derive(Debug, thiserror::Error)]
#[error("The response code ({0:?}) cannot be mapped to our ErrorCode enum")]
pub struct UnmappableStatusCode(wire::message::ResponseStatus);

#[allow(clippy::derive_partial_eq_without_eq)]
mod protos {
    include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));
}

use protos::rpc as wire;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_wire_encoding_roundtrip() {
        let cookie = Cookie::for_namespace(Namespace::from_static("foo"));

        let bytes = cookie.clone().into_wire_encoding();
        let parsed = Cookie::from_wire_encoding(bytes).unwrap();

        assert_eq!(parsed, cookie);
    }

    #[test]
    fn cookie_wire_encoding_length() {
        let cookie = Cookie::for_namespace(Namespace::from_static("foo"));

        let bytes = cookie.into_wire_encoding();

        assert_eq!(bytes.len(), 8 + 3)
    }
}
