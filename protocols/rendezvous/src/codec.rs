use asynchronous_codec::{Encoder, BytesMut, Decoder, Bytes};
use unsigned_varint::codec::UviBytes;
use std::convert::{TryFrom, TryInto};

#[derive(Debug)]
pub enum Message {
    Register {
        namespace: String,
        ttl: Option<i64>,
        // TODO: Signed peer record field
    },
    SuccessfullyRegistered {
        ttl: i64
    },
    FailedToRegister {
        error: ErrorCode,
    },
    Unregister {
        namespace: String,
        // TODO: what is the `id` field here in the PB message
    },
    Discover {
        namespace: Option<String>,
        // TODO limit: Option<i64>
        // TODO cookie: Option<Vec<u8>
    },
    DiscoverResponse {
        registrations: Vec<Registration>,
        // TODO cookie: Option<Vec<u8>
    },
    FailedToDiscover {
        error: ErrorCode
    },
}

#[derive(Debug)]
pub struct Registration {
    namespace: String,
    // ttl: i64, TODO: This is useless as a relative value, need registration timestamp, this needs to be a unix timestamp or this is relative in remaining seconds
    // TODO: Signed peer record
}

#[derive(Debug)]
pub enum ErrorCode {
    InvalidNamespace,
    InvalidSignedPeerRecord,
    InvalidTtl,
    InvalidCookie,
    NotAuthorized,
    InternalError,
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to encode message as bytes")]
    Encode(#[from] prost::EncodeError),
    #[error("Failed to decode message from bytes")]
    Decode(#[from] prost::DecodeError),
    #[error("Failed to read/write")] // TODO: Better message
    Io(#[from] std::io::Error),
    #[error("Failed to convert wire message to internal data model")]
    ConversionError(#[from] ConversionError)
}

impl From<Message> for wire::Message {
    fn from(message: Message) -> Self {
        use wire::message::*;

        match message {
            Message::Register { namespace, ttl } => wire::Message {
                r#type: Some(MessageType::Register.into()),
                register: Some(Register {
                    ns: Some(namespace),
                    ttl,
                    signed_peer_record: None
                }),
                register_response: None,
                unregister: None,
                discover: None,
                discover_response: None
            },
            Message::SuccessfullyRegistered { ttl } => wire::Message {
                r#type: Some(MessageType::RegisterResponse.into()),
                register_response: Some(RegisterResponse {
                    status: Some(ResponseStatus::Ok.into()),
                    status_text: None,
                    ttl: Some(ttl)
                }),
                register: None,
                discover: None,
                unregister: None,
                discover_response: None
            },
            Message::FailedToRegister { error } => wire::Message {
                r#type: Some(MessageType::RegisterResponse.into()),
                register_response: Some(RegisterResponse {
                    status: Some(ResponseStatus::from(error).into()),
                    status_text: None,
                    ttl: None
                }),
                register: None,
                discover: None,
                unregister: None,
                discover_response: None
            },
            Message::Unregister { namespace } => wire::Message {
                r#type: Some(MessageType::Unregister.into()),
                unregister: Some(Unregister {
                    ns: Some(namespace),
                    id: None
                }),
                register: None,
                register_response: None,
                discover: None,
                discover_response: None
            },
            Message::Discover { namespace } => wire::Message {
                r#type: Some(MessageType::Discover.into()),
                discover: Some(Discover {
                    ns: namespace,
                    cookie: None,
                    limit: None
                }),
                register: None,
                register_response: None,
                unregister: None,
                discover_response: None
            },
            Message::DiscoverResponse { registrations } => wire::Message {
                r#type: Some(MessageType::DiscoverResponse.into()),
                discover_response: Some(DiscoverResponse {
                    registrations: registrations.into_iter().map(|reggo| Register {
                        ns: Some(reggo.namespace),
                        ttl: None,
                        signed_peer_record: None
                    }).collect(),
                    status: Some(ResponseStatus::Ok.into()),
                    status_text: None,
                    cookie: None
                }),
                register: None,
                discover: None,
                unregister: None,
                register_response: None
            },
            Message::FailedToDiscover { error } => wire::Message {
                r#type: Some(MessageType::DiscoverResponse.into()),
                discover_response: Some(DiscoverResponse {
                    registrations: Vec::new(),
                    status: Some(ResponseStatus::from(error).into()),
                    status_text: None,
                    cookie: None
                }),
                register: None,
                discover: None,
                unregister: None,
                register_response: None
            },
        }
    }
}

impl TryFrom<wire::Message> for Message {
    type Error = ConversionError;

    fn try_from(message: wire::Message) -> Result<Self, Self::Error> {
        use wire::message::*;

        let message = match message {
            wire::Message { r#type: Some(0), register: Some(Register { ns, ttl, .. }), .. } => {
                Message::Register {
                    namespace: ns.ok_or(ConversionError::MissingNamespace)?,
                    ttl
                }
            },
            wire::Message { r#type: Some(1), register_response: Some(RegisterResponse { status: Some(0), ttl, .. }), .. } => {
                Message::SuccessfullyRegistered {
                    ttl: ttl.ok_or(ConversionError::MissingTtl)?
                }
            },
            wire::Message { r#type: Some(1), register_response: Some(RegisterResponse { status: Some(0), ttl, .. }), .. } => {
                Message::SuccessfullyRegistered {
                    ttl: ttl.ok_or(ConversionError::MissingTtl)?
                }
            },
            wire::Message { r#type: Some(1), register_response: Some(RegisterResponse { status: Some(error_code), .. }), .. } => {
                Message::FailedToRegister {
                    error: wire::message::ResponseStatus::from_i32(error_code).ok_or(ConversionError::BadStatusCode)?.try_into()?
                }
            },
            wire::Message { r#type: Some(2), unregister: Some(Unregister { ns, .. }), .. } => {
                Message::Unregister {
                    namespace: ns.ok_or(ConversionError::MissingNamespace)?
                }
            },
            wire::Message { r#type: Some(3), discover: Some(Discover { ns, .. }), .. } => {
                Message::Discover {
                    namespace: ns
                }
            },
            wire::Message { r#type: Some(4), discover_response: Some(DiscoverResponse { registrations, status: Some(0), .. }), .. } => {
                Message::DiscoverResponse {
                    registrations: registrations.into_iter().map(|reggo| Ok(Registration {
                        namespace: reggo.ns.ok_or(ConversionError::MissingNamespace)?
                    })).collect::<Result<Vec<_>, ConversionError>>()?,
                }
            },
            wire::Message { r#type: Some(4), discover_response: Some(DiscoverResponse { status: Some(error_code), .. }), .. } => {
                let response_status = unsafe {
                    std::mem::transmute::<_, wire::message::ResponseStatus>(error_code)
                };

                Message::FailedToDiscover {
                    error: response_status.try_into()?
                }
            },
            _ => return Err(ConversionError::InconsistentWireMessage)
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
    #[error("Missing TTL field")]
    MissingTtl,
    #[error("Bad status code")]
    BadStatusCode,
}

impl TryFrom<wire::message::ResponseStatus> for ErrorCode {
    type Error = NotAnError;

    fn try_from(value: wire::message::ResponseStatus) -> Result<Self, Self::Error> {
        use wire::message::ResponseStatus::*;

        let code = match value {
            Ok => return Err(NotAnError),
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

impl From<NotAnError> for ConversionError {
    fn from(_: NotAnError) -> Self {
        ConversionError::InconsistentWireMessage
    }
}

#[derive(Debug, thiserror::Error)]
#[error("The provided response code is not an error code")]
pub struct NotAnError;

pub struct RendezvousCodec {
    /// Codec to encode/decode the Unsigned varint length prefix of the frames.
    length_codec: UviBytes,
}

impl Default for RendezvousCodec {
    fn default() -> Self {
        let mut length_codec = UviBytes::default();
        length_codec.set_max_len(1024 * 1024); // 1MB TODO clarify with spec what the default should be

        Self {
            length_codec
        }
    }
}

impl Encoder for RendezvousCodec {
    type Item = Message;
    type Error = Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        use prost::Message;

        let message = wire::Message::from(item);

        let mut buf = Vec::with_capacity(message.encoded_len());

        message.encode(&mut buf)
            .expect("Buffer has sufficient capacity");

        // length prefix the protobuf message, ensuring the max limit is not hit
        self.length_codec
            .encode(Bytes::from(buf), dst)?;

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

mod wire {
    include!(concat!(env!("OUT_DIR"), "/rendezvous.pb.rs"));
}
