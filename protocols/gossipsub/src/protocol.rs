
use constants::*;
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
pub struct GossipsubConfig {
    /// Overlay network parameters.
    /// Number of heartbeats to keep in the `mcache`.
    pub history_length: usize,
    /// Number of past heartbeats to gossip about.
    pub history_gossip: usize,

    /// Target number of peers for the mesh network (D in the spec).
    pub mesh_n: usize,
    /// Minimum number of peers in mesh network before adding more (D_lo
    /// in the spec).
    pub mesh_n_low: usize,
    /// Maximum number of peers in mesh network before removing some
    /// (D_high in the spec).
    pub mesh_n_high: usize,

    /// Number of peers to emit gossip to during a heartbeat (D_lazy in
    /// the spec).
    pub gossip_lazy: usize,

    /// Initial delay in each heartbeat.
    pub heartbeat_initial_delay: Duration,
    /// Time between each heartbeat.
    pub heartbeat_interval: Duration,
    /// Time to live for fanout peers.
    pub fanout_ttl: Duration,
}

impl GossipsubConfig {
    /// Builds a new `GossipsubConfig`.
    #[inline]
    pub fn new() -> GossipsubConfig {
        GossipsubConfig {}
    }
}

impl Default for GossipsubConfig {
    fn default() -> GossipsubConfig {
        GossipsubConfig {
            history_length: GOSSIP_HIST_LEN,
            history_gossip: HISTORY_GOSSIP,
            mesh_n: TARGET_MESH_DEGREE,
            mesh_n_low: LOW_WM_MESH_DEGREE,
            mesh_n_high: HIGH_WM_MESH_DEGREE,
            gossip_lazy: TARGET_MESH_DEGREE, // default to mesh_n
            heartbeat_initial_delay: Duration::from_millis(
                HEARTBEAT_INITIAL_DELAY),
            heartbeat_interval: Duration::from_secs(HEARTBEAT_INTERVAL),
            fanout_ttl: Duration::from_secs(FANOUT_TTL),
        }
    }
}

pub struct GossipsubConfigBuilder {
    history_length: usize,
    /// Number of past heartbeats to gossip about.
    history_gossip: usize,

    /// Target number of peers for the mesh network (D in the spec).
    mesh_n: usize,
    /// Minimum number of peers in mesh network before adding more (D_lo in the spec).
    mesh_n_low: usize,
    /// Maximum number of peers in mesh network before removing some (D_high in the spec).
    mesh_n_high: usize,

    /// Number of peers to emit gossip to during a heartbeat (D_lazy in the spec).
    gossip_lazy: usize,

    /// Initial delay in each heartbeat.
    heartbeat_initial_delay: Duration,
    /// Time between each heartbeat.
    heartbeat_interval: Duration,
    /// Time to live for fanout peers.
    fanout_ttl: Duration,
}

impl Default for GossipsubConfigBuilder {
    fn default() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder {
            history_length: GOSSIP_HIST_LEN,
            history_gossip: HISTORY_GOSSIP,
            mesh_n: TARGET_MESH_DEGREE,
            mesh_n_low: LOW_WM_MESH_DEGREE,
            mesh_n_high: HIGH_WM_MESH_DEGREE,
            gossip_lazy: TARGET_MESH_DEGREE, // default to mesh_n
            heartbeat_initial_delay: Duration::from_millis(
                HEARTBEAT_INITIAL_DELAY),
            heartbeat_interval: Duration::from_secs(HEARTBEAT_INTERVAL),
            fanout_ttl: Duration::from_secs(FANOUT_TTL),
        }
    }
}

impl GossipsubConfigBuilder {
    // set default values
    pub fn new() -> GossipsubConfigBuilder {
        GossipsubConfigBuilder::default()
    }

    pub fn history_length(&mut self, history_length: usize) -> &mut Self {
        assert!(
            history_length >= self.history_gossip,
            "The history_length, {history_length}, must be greater than or equal to the history_gossip length, {self.history_gossip}"
        );
        self.history_length = history_length;
        self
    }

    pub fn history_gossip(&mut self, history_gossip: usize) -> &mut Self {
        assert!(
            self.history_length >= history_gossip,
            "The history_length, {history_length}, must be greater than or equal to the history_gossip length, {self.history_gossip}"
        );
        self.history_gossip = history_gossip;
        self
    }

    pub fn mesh_n(&mut self, mesh_n: usize) -> &mut Self {
        assert!(
            self.mesh_n_low <= mesh_n && mesh_n <= self.mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high. The mesh_n_low is {self.mesh_n_low}, mesh_n is {mesh_n} and mesh_n_high is {self.mesh_n_high}."
        );
        self.mesh_n = mesh_n;
        self
    }

    pub fn mesh_n_low(&mut self, mesh_n_low: usize) -> &mut Self {
        assert!(
            mesh_n_low <= self.mesh_n && self.mesh_n <= self.mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high. The mesh_n_low is {mesh_n_low}, mesh_n is {self.mesh_n} and mesh_n_high is {self.mesh_n_high}."
        );
        self.mesh_ n_low = mesh_n_low;
        self
    }

    pub fn mesh_n_high(&mut self, mesh_n_high: usize) -> &mut Self {
        assert!(
            self.mesh_n_low <= self.mesh_n && self.mesh_n <= mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        self.mesh_n_high = mesh_n_high;
        self
    }

    pub fn gossip_lazy(&mut self, gossip_lazy: usize) -> &mut Self {
        self.gossip_lazy = gossip_lazy;
        self
    }

    pub fn heartbeat_initial_delay(&mut self, heartbeat_initial_delay: Duration) -> &mut Self {
        self.heartbeat_initial_delay = heartbeat_initial_delay;
        self
    }
    pub fn heartbeat_interval(&mut self, heartbeat_interval: Duration) -> &mut Self {
        self.heartbeat_interval = heartbeat_interval;
        self
    }
    pub fn fanout_ttl(&mut self, fanout_ttl: Duration) -> &mut Self {
        self.fanout_ttl = fanout_ttl;
        self
    }

    pub fn build(&self) -> GossipsubConfig {
        GossipsubConfig {
            history_length: self.history_length,
            history_gossip: self.history_gossip,
            mesh_n: self.mesh_n,
            mesh_n_low: self.mesh_n_low,
            mesh_n_high: self.mesh_n_high,
            gossip_lazy: self.gossip_lazy,
            heartbeat_initial_delay: self.heartbeat_initial_delay,
            heartbeat_interval: self.heartbeat_interval,
            fanout_ttl: self.fanout_ttl,
        }
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
                from: PeerId::from_bytes(publish.take_from()).map_err(|_| {
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

