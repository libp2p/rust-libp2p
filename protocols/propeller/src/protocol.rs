//! Propeller protocol definitions and message handling.

use std::{convert::Infallible, pin::Pin};

use asynchronous_codec::{Decoder, Encoder, Framed};
use bytes::BytesMut;
use futures::{future, prelude::*};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::StreamProtocol;
use quick_protobuf_codec::Codec;
use tracing::warn;

use crate::{generated::propeller::pb as proto, message::PropellerMessage};

/// Propeller protocol upgrade for libp2p streams.
#[derive(Debug, Clone)]
pub struct PropellerProtocol {
    protocol_id: StreamProtocol,
    max_message_size: usize,
}

impl PropellerProtocol {
    /// Create a new Propeller protocol.
    pub fn new(protocol_id: StreamProtocol, max_message_size: usize) -> Self {
        Self {
            protocol_id,
            max_message_size,
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
        let codec = PropellerCodec::new(self.max_message_size);
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
        let codec = PropellerCodec::new(self.max_message_size);
        Box::pin(future::ok(Framed::new(socket, codec)))
    }
}

// Propeller codec for the framing

pub struct PropellerCodec {
    /// The codec to handle common encoding/decoding of protobuf messages
    codec: Codec<proto::PropellerMessage>,
}

impl PropellerCodec {
    pub fn new(max_message_size: usize) -> PropellerCodec {
        let codec = Codec::new(max_message_size);
        PropellerCodec { codec }
    }
}

impl Encoder for PropellerCodec {
    type Item<'a> = PropellerMessage;
    type Error = quick_protobuf_codec::Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let proto_message: proto::PropellerMessage = item.into();
        self.codec.encode(proto_message, dst)
    }
}

impl Decoder for PropellerCodec {
    type Item = PropellerMessage;
    type Error = quick_protobuf_codec::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let Some(proto_message) = self.codec.decode(src)? else {
            return Ok(None);
        };

        // Convert from protobuf to our message type
        match PropellerMessage::try_from(proto_message) {
            Ok(message) => Ok(Some(message)),
            Err(e) => {
                warn!("Failed to convert protobuf message: {}", e);
                Ok(None)
            }
        }
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
            data: vec![1, 2, 3, 4, 5],
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
        assert_eq!(orig.data, decoded.data);
        assert_eq!(orig.signature, decoded.signature);
    }
}
