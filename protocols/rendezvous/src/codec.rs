use crate::pow::Difficulty;
use asynchronous_codec::{Bytes, BytesMut, Decoder, Encoder};
use libp2p_core::{peer_record, signed_envelope, PeerRecord, SignedEnvelope};
use rand::RngCore;
use std::convert::{TryFrom, TryInto};
use std::time::SystemTime;
use unsigned_varint::codec::UviBytes;
use uuid::Uuid;

pub type Ttl = i64;

#[derive(Debug, Clone)]
pub enum Message {
    Register(NewRegistration),
    RegisterResponse(Result<Ttl, RegisterErrorResponse>),
    Unregister {
        namespace: String,
    },
    Discover {
        namespace: Option<String>,
        cookie: Option<Cookie>,
    },
    DiscoverResponse(Result<(Vec<Registration>, Cookie), ErrorCode>),
    ProofOfWork {
        hash: [u8; 32],
        nonce: i64,
    },
}

#[derive(Debug, Clone)]
pub enum RegisterErrorResponse {
    Failed(ErrorCode),
    /// We are declining the registration because PoW is required.
    PowRequired {
        challenge: Challenge,
        target: Difficulty,
    },
}

/// A challenge that needs to be incorporated into the PoW hash by the client.
///
/// Sending a challenge to the client ensures the PoW is unique to the registration and the client cannot pre-compute or reuse an existing hash.
#[derive(Debug, Clone)]
pub struct Challenge(Vec<u8>);

impl Challenge {
    pub fn new(rng: &mut impl RngCore) -> Self {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);

        Self(bytes.to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone)]
pub struct Cookie {
    id: Uuid,
    namespace: Option<String>,
}

impl Cookie {
    /// Construct a new [`Cookie`] for a given namespace.
    ///
    /// This cookie will only be valid for subsequent DISCOVER requests targeting the same namespace.
    pub fn for_namespace(namespace: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            namespace: Some(namespace),
        }
    }

    /// Construct a new [`Cookie`] for a DISCOVER request that inquires about all namespaces.
    pub fn for_all_namespaces() -> Self {
        Self {
            id: Uuid::new_v4(),
            namespace: None,
        }
    }

    pub fn into_wire_encoding(self) -> Vec<u8> {
        let namespace = self.namespace.unwrap_or_default();

        let mut buffer = Vec::with_capacity(16 + namespace.len());
        buffer.extend_from_slice(self.id.as_bytes());
        buffer.extend_from_slice(namespace.as_bytes());

        buffer
    }

    pub fn from_wire_encoding(mut bytes: Vec<u8>) -> Result<Self, InvalidCookie> {
        // check length early to avoid panic during slicing
        if bytes.len() < 16 {
            return Err(InvalidCookie);
        }

        let namespace = bytes.split_off(16);
        let namespace = if namespace.len() == 0 {
            None
        } else {
            Some(String::from_utf8(namespace).map_err(|_| InvalidCookie)?)
        };

        let bytes = <[u8; 16]>::try_from(bytes).map_err(|_| InvalidCookie)?;
        let uuid = Uuid::from_bytes(bytes);

        Ok(Self {
            id: uuid,
            namespace,
        })
    }

    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("The cookie was malformed")]
pub struct InvalidCookie;

#[derive(Debug, Clone)]
pub struct NewRegistration {
    pub namespace: String,
    pub record: PeerRecord,
    pub ttl: Option<i64>,
}

/// If unspecified, rendezvous nodes should assume a TTL of 2h.
///
/// See https://github.com/libp2p/specs/blob/d21418638d5f09f2a4e5a1ceca17058df134a300/rendezvous/README.md#L116-L117.
pub const DEFAULT_TTL: i64 = 60 * 60 * 2;

impl NewRegistration {
    pub fn new(namespace: String, record: PeerRecord, ttl: Option<i64>) -> Self {
        Self {
            namespace,
            record,
            ttl,
        }
    }

    pub fn effective_ttl(&self) -> i64 {
        self.ttl.unwrap_or(DEFAULT_TTL)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Registration {
    pub namespace: String,
    pub record: PeerRecord,
    pub ttl: i64,
    pub timestamp: SystemTime,
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
        use prost::Message;

        let message = wire::Message::from(item);

        let mut buf = Vec::with_capacity(message.encoded_len());

        message
            .encode(&mut buf)
            .expect("Buffer has sufficient capacity");

        // length prefix the protobuf message, ensuring the max limit is not hit
        self.length_codec.encode(Bytes::from(buf), dst)?;

        Ok(())
    }
}

impl Decoder for RendezvousCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        use prost::Message;

        let message = match self.length_codec.decode(src)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let message = wire::Message::decode(message)?;

        Ok(Some(message.try_into()?))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to encode message as bytes")]
    Encode(#[from] prost::EncodeError),
    #[error("Failed to decode message from bytes")]
    Decode(#[from] prost::DecodeError),
    #[error("Failed to read/write")]
    Io(#[from] std::io::Error),
    #[error("Failed to convert wire message to internal data model")]
    ConversionError(#[from] ConversionError),
}

impl From<Message> for wire::Message {
    fn from(message: Message) -> Self {
        use wire::message::*;

        match message {
            Message::Register(NewRegistration {
                namespace,
                record,
                ttl,
            }) => wire::Message {
                r#type: Some(MessageType::Register.into()),
                register: Some(Register {
                    ns: Some(namespace),
                    ttl,
                    signed_peer_record: Some(
                        record.into_signed_envelope().into_protobuf_encoding(),
                    ),
                }),
                register_response: None,
                unregister: None,
                discover: None,
                discover_response: None,
                proof_of_work: None,
            },
            Message::RegisterResponse(Ok(ttl)) => wire::Message {
                r#type: Some(MessageType::RegisterResponse.into()),
                register_response: Some(RegisterResponse {
                    status: Some(ResponseStatus::Ok.into()),
                    status_text: None,
                    ttl: Some(ttl),
                    challenge: None,
                    target_difficulty: None,
                }),
                register: None,
                discover: None,
                unregister: None,
                discover_response: None,
                proof_of_work: None,
            },
            Message::RegisterResponse(Err(RegisterErrorResponse::Failed(error))) => wire::Message {
                r#type: Some(MessageType::RegisterResponse.into()),
                register_response: Some(RegisterResponse {
                    status: Some(ResponseStatus::from(error).into()),
                    status_text: None,
                    ttl: None,
                    challenge: None,
                    target_difficulty: None,
                }),
                register: None,
                discover: None,
                unregister: None,
                discover_response: None,
                proof_of_work: None,
            },
            Message::RegisterResponse(Err(RegisterErrorResponse::PowRequired {
                challenge,
                target,
            })) => wire::Message {
                r#type: Some(MessageType::RegisterResponse.into()),
                register_response: Some(RegisterResponse {
                    status: Some(ResponseStatus::EPowRequired.into()),
                    status_text: None,
                    ttl: None,
                    challenge: Some(challenge.0),
                    target_difficulty: Some(target.to_u32()),
                }),
                register: None,
                discover: None,
                unregister: None,
                discover_response: None,
                proof_of_work: None,
            },
            Message::Unregister { namespace } => wire::Message {
                r#type: Some(MessageType::Unregister.into()),
                unregister: Some(Unregister {
                    ns: Some(namespace),
                    id: None,
                }),
                register: None,
                register_response: None,
                discover: None,
                discover_response: None,
                proof_of_work: None,
            },
            Message::Discover { namespace, cookie } => wire::Message {
                r#type: Some(MessageType::Discover.into()),
                discover: Some(Discover {
                    ns: namespace,
                    cookie: cookie.map(|cookie| cookie.into_wire_encoding()),
                    limit: None,
                }),
                register: None,
                register_response: None,
                unregister: None,
                discover_response: None,
                proof_of_work: None,
            },
            Message::DiscoverResponse(Ok((registrations, cookie))) => wire::Message {
                r#type: Some(MessageType::DiscoverResponse.into()),
                discover_response: Some(DiscoverResponse {
                    registrations: registrations
                        .into_iter()
                        .map(|reggo| Register {
                            ns: Some(reggo.namespace),
                            ttl: Some(reggo.ttl),
                            signed_peer_record: Some(
                                reggo.record.into_signed_envelope().into_protobuf_encoding(),
                            ),
                        })
                        .collect(),
                    status: Some(ResponseStatus::Ok.into()),
                    status_text: None,
                    cookie: Some(cookie.into_wire_encoding()),
                }),
                register: None,
                discover: None,
                unregister: None,
                register_response: None,
                proof_of_work: None,
            },
            Message::DiscoverResponse(Err(error)) => wire::Message {
                r#type: Some(MessageType::DiscoverResponse.into()),
                discover_response: Some(DiscoverResponse {
                    registrations: Vec::new(),
                    status: Some(ResponseStatus::from(error).into()),
                    status_text: None,
                    cookie: None,
                }),
                register: None,
                discover: None,
                unregister: None,
                register_response: None,
                proof_of_work: None,
            },
            Message::ProofOfWork { hash, nonce } => wire::Message {
                r#type: Some(MessageType::ProofOfWork.into()),
                proof_of_work: Some(ProofOfWork {
                    hash: Some(hash.to_vec()),
                    nonce: Some(nonce),
                }),
                register: None,
                discover: None,
                discover_response: None,
                unregister: None,
                register_response: None,
            },
        }
    }
}

impl TryFrom<wire::Message> for Message {
    type Error = ConversionError;

    fn try_from(message: wire::Message) -> Result<Self, Self::Error> {
        use wire::message::*;

        let message = match message {
            wire::Message {
                r#type: Some(0),
                register:
                    Some(Register {
                        ns,
                        ttl,
                        signed_peer_record: Some(signed_peer_record),
                    }),
                ..
            } => Message::Register(NewRegistration {
                namespace: ns.ok_or(ConversionError::MissingNamespace)?,
                ttl,
                record: PeerRecord::from_signed_envelope(SignedEnvelope::from_protobuf_encoding(
                    &signed_peer_record,
                )?)?,
            }),
            wire::Message {
                r#type: Some(1),
                register_response:
                    Some(RegisterResponse {
                        status: Some(0),
                        ttl,
                        ..
                    }),
                ..
            } => Message::RegisterResponse(Ok(ttl.ok_or(ConversionError::MissingTtl)?)),
            wire::Message {
                r#type: Some(3),
                discover: Some(Discover { ns, .. }),
                ..
            } => Message::Discover {
                namespace: ns,
                cookie: None,
            },
            wire::Message {
                r#type: Some(4),
                discover_response:
                    Some(DiscoverResponse {
                        registrations,
                        status: Some(0),
                        cookie: Some(cookie),
                        ..
                    }),
                ..
            } => {
                let registrations = registrations
                    .into_iter()
                    .map(|reggo| {
                        Ok(Registration {
                            namespace: reggo.ns.ok_or(ConversionError::MissingNamespace)?,
                            record: PeerRecord::from_signed_envelope(
                                SignedEnvelope::from_protobuf_encoding(
                                    &reggo
                                        .signed_peer_record
                                        .ok_or(ConversionError::MissingSignedPeerRecord)?,
                                )?,
                            )?,
                            ttl: reggo.ttl.ok_or(ConversionError::MissingTtl)?,
                            timestamp: SystemTime::now(),
                        })
                    })
                    .collect::<Result<Vec<_>, ConversionError>>()?;
                let cookie = Cookie::from_wire_encoding(cookie)?;

                Message::DiscoverResponse(Ok((registrations, cookie)))
            }
            wire::Message {
                r#type: Some(1),
                register_response:
                    Some(RegisterResponse {
                        status: Some(error_code),
                        challenge: challenge_bytes,
                        target_difficulty,
                        ..
                    }),
                ..
            } => {
                let error = wire::message::ResponseStatus::from_i32(error_code)
                    .ok_or(ConversionError::BadStatusCode)?;

                let error_response = match (error, challenge_bytes, target_difficulty) {
                    (
                        wire::message::ResponseStatus::EPowRequired,
                        Some(bytes),
                        Some(target_difficulty),
                    ) => RegisterErrorResponse::PowRequired {
                        challenge: Challenge(bytes),
                        target: Difficulty::from_u32(target_difficulty)
                            .ok_or(ConversionError::PoWDifficultyOutOfRange)?,
                    },
                    (code, None, None) => RegisterErrorResponse::Failed(code.try_into()?),
                    _ => return Err(ConversionError::InconsistentWireMessage),
                };

                Message::RegisterResponse(Err(error_response))
            }
            wire::Message {
                r#type: Some(2),
                unregister: Some(Unregister { ns, .. }),
                ..
            } => Message::Unregister {
                namespace: ns.ok_or(ConversionError::MissingNamespace)?,
            },
            wire::Message {
                r#type: Some(4),
                discover_response:
                    Some(DiscoverResponse {
                        status: Some(error_code),
                        ..
                    }),
                ..
            } => {
                let error = wire::message::ResponseStatus::from_i32(error_code)
                    .ok_or(ConversionError::BadStatusCode)?
                    .try_into()?;
                Message::DiscoverResponse(Err(error))
            }
            wire::Message {
                r#type: Some(5),
                proof_of_work:
                    Some(ProofOfWork {
                        hash: Some(hash),
                        nonce: Some(nonce),
                    }),
                ..
            } => Message::ProofOfWork {
                hash: hash.try_into().map_err(|_| ConversionError::BadPoWHash)?,
                nonce,
            },
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
        }
    }
}

impl TryFrom<wire::message::ResponseStatus> for ErrorCode {
    type Error = UnmappableStatusCode;

    fn try_from(value: wire::message::ResponseStatus) -> Result<Self, Self::Error> {
        use wire::message::ResponseStatus::*;

        let code = match value {
            Ok | EPowRequired | EDifficultyTooLow => return Err(UnmappableStatusCode(value)),
            EInvalidNamespace => ErrorCode::InvalidNamespace,
            EInvalidSignedPeerRecord => ErrorCode::InvalidSignedPeerRecord,
            EInvalidTtl => ErrorCode::InvalidTtl,
            EInvalidCookie => ErrorCode::InvalidCookie,
            ENotAuthorized => ErrorCode::NotAuthorized,
            EInternalError => ErrorCode::InternalError,
            EUnavailable => ErrorCode::Unavailable,
        };

        Result::Ok(code)
    }
}

impl From<ErrorCode> for wire::message::ResponseStatus {
    fn from(error_code: ErrorCode) -> Self {
        use wire::message::ResponseStatus::*;

        match error_code {
            ErrorCode::InvalidNamespace => EInvalidNamespace,
            ErrorCode::InvalidSignedPeerRecord => EInvalidSignedPeerRecord,
            ErrorCode::InvalidTtl => EInvalidTtl,
            ErrorCode::InvalidCookie => EInvalidCookie,
            ErrorCode::NotAuthorized => ENotAuthorized,
            ErrorCode::InternalError => EInternalError,
            ErrorCode::Unavailable => EUnavailable,
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

mod wire {
    include!(concat!(env!("OUT_DIR"), "/rendezvous.pb.rs"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_wire_encoding_roundtrip() {
        let cookie = Cookie::for_namespace("foo".to_owned());

        let bytes = cookie.clone().into_wire_encoding();
        let parsed = Cookie::from_wire_encoding(bytes).unwrap();

        assert_eq!(parsed, cookie);
    }

    #[test]
    fn cookie_wire_encoding_length() {
        let cookie = Cookie::for_namespace("foo".to_owned());

        let bytes = cookie.into_wire_encoding();

        assert_eq!(bytes.len(), 16 + 3)
    }
}
