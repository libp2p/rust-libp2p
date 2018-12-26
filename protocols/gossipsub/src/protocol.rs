use message::ControlIHave;
use bytes::{BufMut, Bytes, BytesMut};
use crate::rpc_proto;
use futures::future;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, PeerId};
use libp2p_floodsub::TopicHash;
use message::{ControlMessage, GossipsubMessage, GossipsubSubscription,
    GossipsubSubscriptionAction};
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
    type Output = Framed<TSocket, GossipsubCodec>;
    type Error = io::Error;
    type Future = future::FutureResult<Self::Output, Self::Error>;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::UpgradeId)
        -> Self::Future {
        future::ok(Framed::new(socket, GossipsubCodec {
            length_prefix: Default::default() }))
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for GossipsubConfig
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = Framed<TSocket, GossipsubCodec>;
    type Error = io::Error;
    type Future = future::FutureResult<Self::Output, Self::Error>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::UpgradeId)
        -> Self::Future {
        future::ok(Framed::new(socket, GossipsubCodec {
            length_prefix: Default::default() }))
    }
}

/// Implementation of `tokio_codec::Codec`.
pub struct GossipsubCodec {
    /// The codec for encoding/decoding the length prefix of messages.
    length_prefix: codec::UviBytes,
}

impl Encoder for GossipsubCodec {
    type Item = GossipsubRpc;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut)
        -> Result<(), Self::Error> {
        let mut proto = rpc_proto::RPC::new();

        for message in item.messages.into_iter() {
            let mut msg = rpc_proto::Message::new();
            msg.set_from(message.source.into_bytes());
            msg.set_data(message.data);
            msg.set_seqno(message.sequence_number);
            msg.set_topicIDs(
                message
                    .topics
                    .into_iter()
                    .map(TopicHash::into_string)
                    .collect(),
            );
            msg.set_signature(message.signature);
            msg.set_key(message.key);
            proto.mut_publish().push(msg);
        }

        for topic in item.subscriptions.into_iter() {
            let mut subscription = rpc_proto::RPC_SubOpts::new();
            subscription.set_subscribe(
            topic.action == GossipsubSubscriptionAction::Subscribe);
            subscription.set_topicid(topic.topic.into_string());
            proto.mut_subscriptions().push(subscription);
        }

        for control in item.control.into_iter() {
            let mut ctrl = rpc_proto::ControlMessage::new();
            ctrl.set_ihave(control.ihave);
            ctrl.set_iwant(control.iwant);
            ctrl.set_graft(control.graft);
            ctrl.set_prune(control.prune);
            proto.mut_control().push(control);
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

impl Decoder for GossipsubCodec {
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
            messages.push(GossipsubMessage {
                source: PeerId::from_bytes(publish.take_from()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData,
                    "Invalid peer ID in message")
                })?,
                data: publish.take_data(),
                sequence_number: publish.take_seqno(),
                topics: publish
                    .take_topicIDs()
                    .into_iter()
                    .map(|topic| TopicHash::from_raw(topic))
                    .collect(),
                signature: publish.take_signature(),
                key: publish.take_key(),
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
                    topic: TopicHash::from_raw(sub.take_topicid()),
                })
                .collect(),
        }))
    }
}

/// An RPC received by the Gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<GossipsubMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<GossipsubSubscription>,
    /// Optional control message.
    pub control: ControlMessage,
}
