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

use std::{fmt, io};

use async_trait::async_trait;
use asynchronous_codec::{BytesMut, Decoder, Encoder, FramedRead, FramedWrite};
use futures::{AsyncRead, AsyncWrite, SinkExt, StreamExt};
use libp2p_core::{peer_record, signed_envelope, PeerRecord, SignedEnvelope};
use libp2p_swarm::StreamProtocol;
use quick_protobuf_codec::Codec as ProtobufCodec;
use rand::RngCore;

use crate::DEFAULT_TTL;

pub type Ttl = u64;
pub(crate) type Limit = u64;

const MAX_MESSAGE_LEN_BYTES: usize = 1024 * 1024;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Register(NewRegistration),
    RegisterResponse(Result<Ttl, ErrorCode>),
    Unregister(Namespace),
    Discover {
        namespace: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<Limit>,
    },
    DiscoverResponse(Result<(Vec<Registration>, Cookie), ErrorCode>),
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Namespace(String);

impl Namespace {
    /// Creates a new [`Namespace`] from a static string.
    ///
    /// This will panic if the namespace is too long. We accepting panicking in this case because we
    /// are enforcing a `static lifetime which means this value can only be a constant in the
    /// program and hence we hope the developer checked that it is of an acceptable length.
    pub fn from_static(value: &'static str) -> Self {
        if value.len() > crate::MAX_NAMESPACE {
            panic!("Namespace '{value}' is too long!")
        }

        Namespace(value.to_owned())
    }

    pub fn new(value: String) -> Result<Self, NamespaceTooLong> {
        if value.len() > crate::MAX_NAMESPACE {
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
    /// This cookie will only be valid for subsequent DISCOVER requests targeting the same
    /// namespace.
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

#[derive(Debug, Clone, PartialEq)]
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

impl Encoder for Codec {
    type Item<'a> = Message;
    type Error = Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut pb: ProtobufCodec<proto::Message> = ProtobufCodec::new(MAX_MESSAGE_LEN_BYTES);

        pb.encode(proto::Message::from(item), dst)?;

        Ok(())
    }
}

impl Decoder for Codec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut pb: ProtobufCodec<proto::Message> = ProtobufCodec::new(MAX_MESSAGE_LEN_BYTES);

        let Some(message) = pb.decode(src)? else {
            return Ok(None);
        };

        Ok(Some(message.try_into()?))
    }
}

#[derive(Clone, Default)]
pub struct Codec {}

#[async_trait]
impl libp2p_request_response::Codec for Codec {
    type Protocol = StreamProtocol;
    type Request = Message;
    type Response = Message;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let message = FramedRead::new(io, self.clone())
            .next()
            .await
            .ok_or(io::ErrorKind::UnexpectedEof)??;

        Ok(message)
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let message = FramedRead::new(io, self.clone())
            .next()
            .await
            .ok_or(io::ErrorKind::UnexpectedEof)??;

        Ok(message)
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        FramedWrite::new(io, self.clone()).send(req).await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        FramedWrite::new(io, self.clone()).send(res).await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("Failed to read/write")]
    Io(#[from] std::io::Error),
    #[error("Failed to convert wire message to internal data model")]
    Conversion(#[from] ConversionError),
}

impl From<Error> for std::io::Error {
    fn from(value: Error) -> Self {
        match value {
            Error::Io(e) => e,
            Error::Codec(e) => io::Error::from(e),
            Error::Conversion(e) => io::Error::new(io::ErrorKind::InvalidInput, e),
        }
    }
}

impl From<Message> for proto::Message {
    fn from(message: Message) -> Self {
        match message {
            Message::Register(NewRegistration {
                namespace,
                record,
                ttl,
            }) => proto::Message {
                type_pb: Some(proto::MessageType::REGISTER),
                register: Some(proto::Register {
                    ns: Some(namespace.into()),
                    ttl,
                    signedPeerRecord: Some(record.into_signed_envelope().into_protobuf_encoding()),
                }),
                registerResponse: None,
                unregister: None,
                discover: None,
                discoverResponse: None,
            },
            Message::RegisterResponse(Ok(ttl)) => proto::Message {
                type_pb: Some(proto::MessageType::REGISTER_RESPONSE),
                registerResponse: Some(proto::RegisterResponse {
                    status: Some(proto::ResponseStatus::OK),
                    statusText: None,
                    ttl: Some(ttl),
                }),
                register: None,
                discover: None,
                unregister: None,
                discoverResponse: None,
            },
            Message::RegisterResponse(Err(error)) => proto::Message {
                type_pb: Some(proto::MessageType::REGISTER_RESPONSE),
                registerResponse: Some(proto::RegisterResponse {
                    status: Some(proto::ResponseStatus::from(error)),
                    statusText: None,
                    ttl: None,
                }),
                register: None,
                discover: None,
                unregister: None,
                discoverResponse: None,
            },
            Message::Unregister(namespace) => proto::Message {
                type_pb: Some(proto::MessageType::UNREGISTER),
                unregister: Some(proto::Unregister {
                    ns: Some(namespace.into()),
                    id: None,
                }),
                register: None,
                registerResponse: None,
                discover: None,
                discoverResponse: None,
            },
            Message::Discover {
                namespace,
                cookie,
                limit,
            } => proto::Message {
                type_pb: Some(proto::MessageType::DISCOVER),
                discover: Some(proto::Discover {
                    ns: namespace.map(|ns| ns.into()),
                    cookie: cookie.map(|cookie| cookie.into_wire_encoding()),
                    limit,
                }),
                register: None,
                registerResponse: None,
                unregister: None,
                discoverResponse: None,
            },
            Message::DiscoverResponse(Ok((registrations, cookie))) => proto::Message {
                type_pb: Some(proto::MessageType::DISCOVER_RESPONSE),
                discoverResponse: Some(proto::DiscoverResponse {
                    registrations: registrations
                        .into_iter()
                        .map(|reggo| proto::Register {
                            ns: Some(reggo.namespace.into()),
                            ttl: Some(reggo.ttl),
                            signedPeerRecord: Some(
                                reggo.record.into_signed_envelope().into_protobuf_encoding(),
                            ),
                        })
                        .collect(),
                    status: Some(proto::ResponseStatus::OK),
                    statusText: None,
                    cookie: Some(cookie.into_wire_encoding()),
                }),
                register: None,
                discover: None,
                unregister: None,
                registerResponse: None,
            },
            Message::DiscoverResponse(Err(error)) => proto::Message {
                type_pb: Some(proto::MessageType::DISCOVER_RESPONSE),
                discoverResponse: Some(proto::DiscoverResponse {
                    registrations: Vec::new(),
                    status: Some(proto::ResponseStatus::from(error)),
                    statusText: None,
                    cookie: None,
                }),
                register: None,
                discover: None,
                unregister: None,
                registerResponse: None,
            },
        }
    }
}

impl TryFrom<proto::Message> for Message {
    type Error = ConversionError;

    fn try_from(message: proto::Message) -> Result<Self, Self::Error> {
        let message = match message {
            proto::Message {
                type_pb: Some(proto::MessageType::REGISTER),
                register:
                    Some(proto::Register {
                        ns,
                        ttl,
                        signedPeerRecord: Some(signed_peer_record),
                    }),
                ..
            } => Message::Register(NewRegistration {
                namespace: ns
                    .map(Namespace::new)
                    .transpose()?
                    .ok_or(ConversionError::MissingNamespace)?,
                ttl,
                record: PeerRecord::from_signed_envelope(SignedEnvelope::from_protobuf_encoding(
                    &signed_peer_record,
                )?)?,
            }),
            proto::Message {
                type_pb: Some(proto::MessageType::REGISTER_RESPONSE),
                registerResponse:
                    Some(proto::RegisterResponse {
                        status: Some(proto::ResponseStatus::OK),
                        ttl,
                        ..
                    }),
                ..
            } => Message::RegisterResponse(Ok(ttl.ok_or(ConversionError::MissingTtl)?)),
            proto::Message {
                type_pb: Some(proto::MessageType::DISCOVER),
                discover: Some(proto::Discover { ns, limit, cookie }),
                ..
            } => Message::Discover {
                namespace: ns.map(Namespace::new).transpose()?,
                cookie: cookie.map(Cookie::from_wire_encoding).transpose()?,
                limit,
            },
            proto::Message {
                type_pb: Some(proto::MessageType::DISCOVER_RESPONSE),
                discoverResponse:
                    Some(proto::DiscoverResponse {
                        registrations,
                        status: Some(proto::ResponseStatus::OK),
                        cookie: Some(cookie),
                        ..
                    }),
                ..
            } => {
                let registrations = registrations
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
                let cookie = Cookie::from_wire_encoding(cookie)?;

                Message::DiscoverResponse(Ok((registrations, cookie)))
            }
            proto::Message {
                type_pb: Some(proto::MessageType::REGISTER_RESPONSE),
                registerResponse:
                    Some(proto::RegisterResponse {
                        status: Some(response_status),
                        ..
                    }),
                ..
            } => Message::RegisterResponse(Err(response_status.try_into()?)),
            proto::Message {
                type_pb: Some(proto::MessageType::UNREGISTER),
                unregister: Some(proto::Unregister { ns, .. }),
                ..
            } => Message::Unregister(
                ns.map(Namespace::new)
                    .transpose()?
                    .ok_or(ConversionError::MissingNamespace)?,
            ),
            proto::Message {
                type_pb: Some(proto::MessageType::DISCOVER_RESPONSE),
                discoverResponse:
                    Some(proto::DiscoverResponse {
                        status: Some(response_status),
                        ..
                    }),
                ..
            } => Message::DiscoverResponse(Err(response_status.try_into()?)),
            _ => return Err(ConversionError::InconsistentWireMessage),
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

impl TryFrom<proto::ResponseStatus> for ErrorCode {
    type Error = UnmappableStatusCode;

    fn try_from(value: proto::ResponseStatus) -> Result<Self, Self::Error> {
        use proto::ResponseStatus::*;

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

        Ok(code)
    }
}

impl From<ErrorCode> for proto::ResponseStatus {
    fn from(error_code: ErrorCode) -> Self {
        use proto::ResponseStatus::*;

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
pub struct UnmappableStatusCode(proto::ResponseStatus);

mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub(crate) use self::rendezvous::pb::{mod_Message::*, Message};
}

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
