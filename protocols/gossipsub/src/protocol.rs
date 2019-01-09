use message::{GMessage, GossipsubSubscription,
    GossipsubSubscriptionAction, GossipsubRpc, ControlMessage};
use {TopicHash};

use bytes::{BufMut, BytesMut};
use crate::rpc_proto;
use futures::future;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, PeerId};
use protobuf::Message as ProtobufMessage;
use std::{io, iter};
use tokio_codec::{Decoder, Encoder, Framed};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

/// Implementation of `ConnectionUpgrade` for the Gossipsub protocol.
#[derive(Debug, Clone)]
pub struct GossipsubConfig {}

impl GossipsubConfig {
    /// Builds a new `GossipsubConfig`.
    #[inline]
    pub fn new() -> GossipsubConfig {
        GossipsubConfig {}
    }
}

impl UpgradeInfo for GossipsubConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    #[inline]
    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/gossipsub/1.0.0")
    }
}

impl<TSocket> InboundUpgrade<TSocket> for GossipsubConfig
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = Framed<TSocket, GossipsubRpcCodec>;
    type Error = io::Error;
    type Future = future::FutureResult<Self::Output, Self::Error>;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::Info)
        -> Self::Future {
        future::ok(Framed::new(socket, GossipsubRpcCodec {
            length_prefix: Default::default() }))
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for GossipsubConfig
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = Framed<TSocket, GossipsubRpcCodec>;
    type Error = io::Error;
    type Future = future::FutureResult<Self::Output, Self::Error>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info)
        -> Self::Future {
        future::ok(Framed::new(socket, GossipsubRpcCodec {
            length_prefix: Default::default() }))
    }
}

/// Implementation of `tokio_codec::Codec` for a `message::GossipsubRpc` to an
/// `rpc_proto::RPC`.
pub struct GossipsubRpcCodec {
    /// The codec for encoding/decoding the length prefix of messages.
    length_prefix: codec::UviBytes,
}

impl Encoder for GossipsubRpcCodec {
    type Item = GossipsubRpc;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut)
        -> Result<(), Self::Error> {
        let mut proto = rpc_proto::RPC::new();

        for message in item.messages.into_iter() {
            let msg = rpc_proto::Message::from(message);
            proto.mut_publish().push(msg);
        }

        for gsub in item.subscriptions.into_iter() {
            let mut subscription = rpc_proto::RPC_SubOpts::from(gsub);
            proto.mut_subscriptions().push(subscription);
        }

        for control in item.control.into_iter() {
            let mut ctrl = rpc_proto::ControlMessage::from(control);

            proto.set_control(ctrl);
        }

        let msg_size = proto.compute_size();

        // Reserve enough space for the data and the length. The length has a
        // maximum of 32 bits, which means that 5 bytes is enough for the
        // variable-length integer.
        dst.reserve(msg_size as usize + 5);

        proto
            .write_length_delimited_to_writer(&mut dst.by_ref().writer())
            .expect(
                "there is no situation in which the protobuf message can be \
                invalid, and writing to a BytesMut never fails as we \
                reserved enough space beforehand",
            );
        Ok(())
    }
}

impl Decoder for GossipsubRpcCodec {
    type Item = GossipsubRpc;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut)
        -> Result<Option<Self::Item>, Self::Error> {
        let packet = match self.length_prefix.decode(src)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let mut rpc: rpc_proto::RPC = protobuf::parse_from_bytes(&packet)?;

        let mut messages = Vec::with_capacity(rpc.get_publish().len());
        for mut publish in rpc.take_publish().into_iter() {
            messages.push(GMessage {
                source: PeerId::from_bytes(publish.take_from()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData,
                    "Invalid peer ID in message")
                })?,
                data: publish.take_data(),
                seq_no: publish.take_seqno(),
                topics: publish
                    .take_topic_hashes()
                    .into_iter()
                    .map(|topic| TopicHash::from_raw(topic))
                    .collect(),
                // signature: publish.take_signature(),
                // key: publish.take_key(),
            });
        }

        Ok(Some(GossipsubRpc {
            messages,
            subscriptions: rpc
                .take_subscriptions()
                .into_iter()
                .map(|mut sub| GossipsubSubscription {
                    action: if sub.get_subscribe() {
                        GossipsubSubscriptionAction::Subscribe
                    } else {
                        GossipsubSubscriptionAction::Unsubscribe
                    },
                    topic: TopicHash::from_raw(sub.take_topic_hash()),
                })
                .collect(),
            control: Some(ControlMessage::from(rpc.take_control())),
        }))
    }
}

