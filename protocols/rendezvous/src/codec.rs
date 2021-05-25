use asynchronous_codec::{Encoder, BytesMut, Decoder, Bytes};
use unsigned_varint::codec::UviBytes;

/// The event emitted by the Handler. This informs the behaviour of various events created
/// by the handler.
#[derive(Debug)]
pub enum Message {
    Register {
        namespace: String
    },
    Discover {
        namespace: Option<String>
    },
    SuccessfullyRegistered {
        ttl: i64
    },
    FailedtoRegister {
        error: ErrorCode,
    },
    DiscoverResp,
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
    Io(#[from] std::io::Error)
}

impl From<Message> for wire::Message {
    fn from(_: Message) -> Self {
        todo!()
    }
}

impl From<wire::Message> for Message {
    fn from(_: wire::Message) -> Self {
        todo!()
    }
}

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

        Ok(Some(message.into()))
    }
}

mod wire {
    include!(concat!(env!("OUT_DIR"), "/rendezvous.pb.rs"));
}
