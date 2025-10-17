//! Propeller protocol definitions and message handling.

use std::{convert::Infallible, pin::Pin};

use asynchronous_codec::{Decoder, Encoder, Framed};
use bytes::BytesMut;
use futures::{future, prelude::*};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::StreamProtocol;
use quick_protobuf_codec::Codec;

use crate::{generated::propeller::pb as proto, message::PropellerMessage};

/// Propeller protocol upgrade for libp2p streams.
#[derive(Debug, Clone)]
pub struct PropellerProtocol {
    protocol_id: StreamProtocol,
    max_shred_size: usize,
}

impl PropellerProtocol {
    /// Create a new Propeller protocol.
    pub fn new(protocol_id: StreamProtocol, max_shred_size: usize) -> Self {
        Self {
            protocol_id,
            max_shred_size,
        }
    }
}

impl UpgradeInfo for PropellerProtocol {
    type Info = StreamProtocol;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(self.protocol_id.clone())
    }
}

impl<TSocket> InboundUpgrade<TSocket> for PropellerProtocol
where
    TSocket: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = Framed<TSocket, PropellerCodec>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let codec = PropellerCodec::new(self.max_shred_size);
        Box::pin(future::ok(Framed::new(socket, codec)))
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for PropellerProtocol
where
    TSocket: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = Framed<TSocket, PropellerCodec>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let codec = PropellerCodec::new(self.max_shred_size);
        Box::pin(future::ok(Framed::new(socket, codec)))
    }
}

// Propeller codec for the framing

pub struct PropellerCodec {
    /// The codec to handle common encoding/decoding of protobuf messages
    codec: Codec<proto::PropellerMessage>,
}

impl PropellerCodec {
    pub fn new(max_shred_size: usize) -> PropellerCodec {
        let codec = Codec::new(max_shred_size);
        PropellerCodec { codec }
    }
}

impl Encoder for PropellerCodec {
    type Item<'a> = PropellerMessage;
    type Error = quick_protobuf_codec::Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let msg_id = item.shred.id.message_id;
        let index = item.shred.id.index;
        let publisher = item.shred.id.publisher;
        let shard_len = item.shred.shard.len();
        let sig_len = item.shred.signature.len();

        let proto_message: proto::PropellerMessage = item.into();

        match self.codec.encode(proto_message, dst) {
            Ok(()) => Ok(()),
            Err(e) => {
                tracing::error!(
                    "Failed to encode message: error={}, msg_id={}, index={}, publisher={}, shard_len={}, sig_len={}, dst_len={}, dst_capacity={}",
                    e, msg_id, index, publisher, shard_len, sig_len, dst.len(), dst.capacity()
                );
                Err(e)
            }
        }
    }
}

impl Decoder for PropellerCodec {
    type Item = PropellerMessage;
    type Error = quick_protobuf_codec::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let src_len = src.len();

        let proto_message = match self.codec.decode(src) {
            Ok(Some(msg)) => msg,
            Ok(None) => return Ok(None),
            Err(e) => {
                tracing::warn!(
                    "Failed to decode protobuf: error={}, src_len={}, remaining={}",
                    e,
                    src_len,
                    src.len()
                );
                return Err(e);
            }
        };

        // Convert from protobuf to our message type
        let message = match PropellerMessage::try_from(proto_message) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::warn!(
                    "Failed to convert protobuf message: error={}, src_len={}",
                    e,
                    src_len
                );
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid protobuf message: {}", e),
                )
                .into());
            }
        };
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use asynchronous_codec::{Decoder, Encoder};
    use bytes::BytesMut;
    use libp2p_core::PeerId;

    use super::*;
    use crate::message::{PropellerMessage, Shred, ShredId};

    #[test]
    fn test_propeller_codec_roundtrip() {
        let mut codec = PropellerCodec::new(65536);
        let mut buffer = BytesMut::new();

        let shred = Shred {
            id: ShredId {
                message_id: 100,
                index: 5,
                publisher: PeerId::random(),
            },
            shard: vec![1, 2, 3, 4, 5],
            signature: vec![0; 64],
        };

        let original_message = PropellerMessage {
            shred: shred.clone(),
        };

        // Encode
        codec.encode(original_message.clone(), &mut buffer).unwrap();

        // Decode
        let decoded_message = codec.decode(&mut buffer).unwrap().unwrap();

        // Verify
        let orig = &original_message.shred;
        let decoded = &decoded_message.shred;
        assert_eq!(orig.id.message_id, decoded.id.message_id);
        assert_eq!(orig.id.index, decoded.id.index);
        assert_eq!(orig.id.publisher, decoded.id.publisher);
        assert_eq!(orig.shard, decoded.shard);
        assert_eq!(orig.signature, decoded.signature);
    }
}
