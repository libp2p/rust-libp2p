// Copyright 2020 Sigma Prime Pty Ltd.
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

use std::{collections::HashMap, convert::Infallible, io, pin::Pin};

use asynchronous_codec::{Decoder, Encoder, Framed};
use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use futures::prelude::*;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_identity::{PeerId, PublicKey};
use libp2p_swarm::StreamProtocol;
use prost::Message;
use prost::encoding::{DecodeContext, decode_key, skip_field};
use prost_codec::consume_message_prefix;

#[cfg(feature = "partial-messages")]
use crate::extensions::partial_messages::PartialMessage;
use crate::{
    ValidationError,
    config::ValidationMode,
    handler::HandlerEvent,
    rpc_proto::proto,
    topic::TopicHash,
    types::{
        ControlAction, Extensions, Graft, IDontWant, IHave, IWant, MessageId, PeerInfo, PeerKind,
        Prune, RawMessage, RpcIn, Subscription, SubscriptionAction, SubscriptionOpts,
    },
};

pub(crate) const SIGNING_PREFIX: &[u8] = b"libp2p-pubsub:";

pub(crate) const GOSSIPSUB_1_3_0_PROTOCOL: ProtocolId = ProtocolId {
    protocol: StreamProtocol::new("/meshsub/1.3.0"),
    kind: PeerKind::Gossipsubv1_3,
};

pub(crate) const GOSSIPSUB_1_2_0_PROTOCOL: ProtocolId = ProtocolId {
    protocol: StreamProtocol::new("/meshsub/1.2.0"),
    kind: PeerKind::Gossipsubv1_2,
};

pub(crate) const GOSSIPSUB_1_1_0_PROTOCOL: ProtocolId = ProtocolId {
    protocol: StreamProtocol::new("/meshsub/1.1.0"),
    kind: PeerKind::Gossipsubv1_1,
};
pub(crate) const GOSSIPSUB_1_0_0_PROTOCOL: ProtocolId = ProtocolId {
    protocol: StreamProtocol::new("/meshsub/1.0.0"),
    kind: PeerKind::Gossipsub,
};
pub(crate) const FLOODSUB_PROTOCOL: ProtocolId = ProtocolId {
    protocol: StreamProtocol::new("/floodsub/1.0.0"),
    kind: PeerKind::Floodsub,
};

/// Implementation of [`InboundUpgrade`] and [`OutboundUpgrade`] for the Gossipsub protocol.
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    /// The Gossipsub protocol id to listen on.
    pub(crate) protocol_ids: Vec<ProtocolId>,
    /// Determines the level of validation to be done on incoming messages.
    pub(crate) validation_mode: ValidationMode,
    /// The default max transmit size.
    pub(crate) default_max_transmit_size: usize,
    /// The max transmit sizes for a topic.
    pub(crate) max_transmit_sizes: HashMap<TopicHash, usize>,
    /// The max number of publish messages to decode in a single RPC.
    pub(crate) max_publish_messages: usize,
    /// The max byte size of each control message (IHAVE/IWANT/IDONTWANT/GRAFT/PRUNE) and
    /// subscription in a single RPC. Messages exceeding this size will be rejected.
    pub(crate) max_control_message_size: usize,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            validation_mode: ValidationMode::Strict,
            protocol_ids: vec![
                GOSSIPSUB_1_3_0_PROTOCOL,
                GOSSIPSUB_1_2_0_PROTOCOL,
                GOSSIPSUB_1_1_0_PROTOCOL,
                GOSSIPSUB_1_0_0_PROTOCOL,
            ],
            default_max_transmit_size: 65536,
            max_transmit_sizes: HashMap::new(),
            max_publish_messages: 500,
            max_control_message_size: 5120, // 5KB
        }
    }
}

impl ProtocolConfig {
    /// Get the max transmit size for a given topic, falling back to the default.
    pub fn max_transmit_size_for_topic(&self, topic: &TopicHash) -> usize {
        self.max_transmit_sizes
            .get(topic)
            .copied()
            .unwrap_or(self.default_max_transmit_size)
    }
}

/// The protocol ID
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolId {
    /// The RPC message type/name.
    pub protocol: StreamProtocol,
    /// The type of protocol we support
    pub kind: PeerKind,
}

impl AsRef<str> for ProtocolId {
    fn as_ref(&self) -> &str {
        self.protocol.as_ref()
    }
}

impl UpgradeInfo for ProtocolConfig {
    type Info = ProtocolId;
    type InfoIter = Vec<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.protocol_ids.clone()
    }
}

impl<TSocket> InboundUpgrade<TSocket> for ProtocolConfig
where
    TSocket: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (Framed<TSocket, GossipsubCodec>, PeerKind);
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: TSocket, protocol_id: Self::Info) -> Self::Future {
        Box::pin(future::ok((
            Framed::new(
                socket,
                GossipsubCodec::new(
                    self.default_max_transmit_size,
                    self.validation_mode,
                    self.max_transmit_sizes,
                    self.max_publish_messages,
                    self.max_control_message_size,
                ),
            ),
            protocol_id.kind,
        )))
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for ProtocolConfig
where
    TSocket: AsyncWrite + AsyncRead + Unpin + Send + 'static,
{
    type Output = (Framed<TSocket, GossipsubCodec>, PeerKind);
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: TSocket, protocol_id: Self::Info) -> Self::Future {
        Box::pin(future::ok((
            Framed::new(
                socket,
                GossipsubCodec::new(
                    self.default_max_transmit_size,
                    self.validation_mode,
                    self.max_transmit_sizes,
                    self.max_publish_messages,
                    self.max_control_message_size,
                ),
            ),
            protocol_id.kind,
        )))
    }
}

// Gossip codec for the framing

pub struct GossipsubCodec {
    /// Determines the level of validation performed on incoming messages.
    validation_mode: ValidationMode,
    /// The codec to handle common encoding/decoding of protobuf messages
    codec: prost_codec::Codec<proto::Rpc>,
    /// Maximum transmit sizes per topic, with a default if not specified.
    max_transmit_sizes: HashMap<TopicHash, usize>,
    /// The max number of publish messages to decode in a single RPC.
    max_publish_messages: usize,
    /// The max byte size of each control message (IHAVE/IWANT/IDONTWANT/GRAFT/PRUNE) and
    /// subscription in a single RPC. Messages exceeding this size will be rejected.
    max_control_message_size: usize,
}

impl GossipsubCodec {
    pub fn new(
        max_length: usize,
        validation_mode: ValidationMode,
        max_transmit_sizes: HashMap<TopicHash, usize>,
        max_publish_messages: usize,
        max_control_message_size: usize,
    ) -> GossipsubCodec {
        let codec = prost_codec::Codec::new(max_length);
        GossipsubCodec {
            validation_mode,
            codec,
            max_transmit_sizes,
            max_publish_messages,
            max_control_message_size,
        }
    }

    /// Get the max transmit size for a given topic if it exists.
    fn max_transmit_size_for_topic(&self, topic: &TopicHash) -> Option<usize> {
        self.max_transmit_sizes.get(topic).copied()
    }

    /// Verifies a gossipsub message. This returns either a success or failure. All errors
    /// are logged, which prevents error handling in the codec and handler. We simply drop invalid
    /// messages and log warnings, rather than propagating errors through the codec.
    fn verify_signature(message: &proto::Message) -> bool {
        let Some(from) = message.from.as_ref() else {
            tracing::debug!("Signature verification failed: No source id given");
            return false;
        };

        let Ok(source) = PeerId::from_bytes(from) else {
            tracing::debug!("Signature verification failed: Invalid Peer Id");
            return false;
        };

        let Some(signature) = message.signature.as_ref() else {
            tracing::debug!("Signature verification failed: No signature provided");
            return false;
        };

        // If there is a key value in the protobuf, use that key otherwise the key must be
        // obtained from the inlined source peer_id.
        let public_key = match message.key.as_deref().map(PublicKey::try_decode_protobuf) {
            Some(Ok(key)) => key,
            _ => match PublicKey::try_decode_protobuf(&source.to_bytes()[2..]) {
                Ok(v) => v,
                Err(_) => {
                    tracing::warn!("Signature verification failed: No valid public key supplied");
                    return false;
                }
            },
        };

        // The key must match the peer_id
        if source != public_key.to_peer_id() {
            tracing::warn!(
                "Signature verification failed: Public key doesn't match source peer id"
            );
            return false;
        }

        // Construct the signature bytes
        let mut message_sig = message.clone();
        message_sig.signature = None;
        message_sig.key = None;
        let buf = message_sig.encode_to_vec();
        let mut signature_bytes = SIGNING_PREFIX.to_vec();
        signature_bytes.extend_from_slice(&buf);
        public_key.verify(&signature_bytes, signature)
    }
}

impl Encoder for GossipsubCodec {
    type Item<'a> = proto::Rpc;
    type Error = prost_codec::Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.codec.encode(item, dst)
    }
}

/// Validate RPC limits by parsing the wire format without allocating.
///
/// Note: `src` should be the message bytes with the length prefix.
fn validate_rpc_limits(
    mut buf: &[u8],
    max_publish_messages: usize,
    max_control_message_size: usize,
) -> io::Result<()> {
    // Consume length prefix and get message bytes from length-prefixed buffer for validation
    consume_message_prefix(&mut buf)?;

    let mut publish_count = 0;
    let mut control_size = 0;
    while !buf.is_empty() {
        let field_start = buf;
        let (tag, wire_type) =
            decode_key(&mut buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        skip_field(wire_type, tag, &mut buf, DecodeContext::default())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        match tag {
            // Publish (2) - count messages
            2 => {
                publish_count += 1;
                if publish_count > max_publish_messages {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "too many publish messages",
                    ));
                }
            }
            // Control message - validate and accumulate size
            1 | 3 => {
                let field_size = field_start.len() - buf.len();
                control_size += field_size;
                if control_size > max_control_message_size {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "rpc control size exceeds max control message size",
                    ));
                }
            }
            // Unknown fields - skip
            _ => {}
        }
    }
    Ok(())
}

impl Decoder for GossipsubCodec {
    type Item = HandlerEvent;
    type Error = prost_codec::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Pre-validate: discard if limits exceeded
        validate_rpc_limits(
            src.as_ref(),
            self.max_publish_messages,
            self.max_control_message_size,
        )?;

        // Safe to decode with prost
        let Some(mut rpc) = self.codec.decode(src)? else {
            return Ok(None);
        };

        // Store valid messages.
        let mut messages = Vec::with_capacity(rpc.publish.len());
        // Store any invalid messages.
        let mut invalid_messages = Vec::new();

        for message in rpc.publish.into_iter() {
            let topic = TopicHash::from_raw(&message.topic);

            // Check the message size to ensure it doesn't bypass the configured max.
            if self
                .max_transmit_size_for_topic(&topic)
                .is_some_and(|max| message.encoded_len() > max)
            {
                let message = RawMessage {
                    source: None, // don't bother inform the application
                    data: message.data.unwrap_or_default(),
                    sequence_number: None, // don't inform the application
                    topic: TopicHash::from_raw(message.topic),
                    signature: None, // don't inform the application
                    key: message.key,
                    validated: false,
                };

                invalid_messages.push((message, ValidationError::MessageSizeTooLargeForTopic));
                continue;
            }

            // Keep track of the type of invalid message.
            let mut invalid_kind = None;
            let mut verify_signature = false;
            let mut verify_sequence_no = false;
            let mut verify_source = false;

            match self.validation_mode {
                ValidationMode::Strict => {
                    // Validate everything
                    verify_signature = true;
                    verify_sequence_no = true;
                    verify_source = true;
                }
                ValidationMode::Permissive => {
                    // If the fields exist, validate them
                    if message.signature.is_some() {
                        verify_signature = true;
                    }
                    if message.seqno.is_some() {
                        verify_sequence_no = true;
                    }
                    if message.from.is_some() {
                        verify_source = true;
                    }
                }
                ValidationMode::Anonymous => {
                    if message.signature.is_some() {
                        tracing::warn!(
                            "Signature field was non-empty and anonymous validation mode is set"
                        );
                        invalid_kind = Some(ValidationError::SignaturePresent);
                    } else if message.seqno.is_some() {
                        tracing::warn!(
                            "Sequence number was non-empty and anonymous validation mode is set"
                        );
                        invalid_kind = Some(ValidationError::SequenceNumberPresent);
                    } else if message.from.is_some() {
                        tracing::warn!(
                            "Message dropped. Message source was non-empty and anonymous validation mode is set"
                        );
                        invalid_kind = Some(ValidationError::MessageSourcePresent);
                    }
                }
                ValidationMode::None => {}
            }

            // If the initial validation logic failed, add the message to invalid messages and
            // continue processing the others.
            if let Some(validation_error) = invalid_kind.take() {
                let message = RawMessage {
                    source: None, // don't bother inform the application
                    data: message.data.unwrap_or_default(),
                    sequence_number: None, // don't inform the application
                    topic: TopicHash::from_raw(message.topic),
                    signature: None, // don't inform the application
                    key: message.key,
                    validated: false,
                };
                invalid_messages.push((message, validation_error));
                // proceed to the next message
                continue;
            }

            // verify message signatures if required
            if verify_signature && !GossipsubCodec::verify_signature(&message) {
                tracing::warn!("Invalid signature for received message");
                // Build the invalid message (ignoring further validation of sequence number
                // and source)
                let message = RawMessage {
                    source: None, // don't bother inform the application
                    data: message.data.unwrap_or_default(),
                    sequence_number: None, // don't inform the application
                    topic: TopicHash::from_raw(message.topic),
                    signature: None, // don't inform the application
                    key: message.key,
                    validated: false,
                };
                invalid_messages.push((message, ValidationError::InvalidSignature));
                // proceed to the next message
                continue;
            }

            // ensure the sequence number is a u64
            let sequence_number = if verify_sequence_no {
                if let Some(seq_no) = message.seqno {
                    if seq_no.is_empty() {
                        None
                    } else if seq_no.len() != 8 {
                        tracing::debug!(
                            sequence_number=?seq_no,
                            sequence_length=%seq_no.len(),
                            "Invalid sequence number length for received message"
                        );

                        let message = RawMessage {
                            source: None, // don't bother inform the application
                            data: message.data.unwrap_or_default(),
                            sequence_number: None, // don't inform the application
                            topic: TopicHash::from_raw(message.topic),
                            signature: message.signature, // don't inform the application
                            key: message.key,
                            validated: false,
                        };
                        invalid_messages.push((message, ValidationError::InvalidSequenceNumber));
                        // proceed to the next message
                        continue;
                    } else {
                        // valid sequence number
                        Some(BigEndian::read_u64(&seq_no))
                    }
                } else {
                    // sequence number was not present
                    tracing::debug!("Sequence number not present but expected");
                    let message = RawMessage {
                        source: None, // don't bother inform the application
                        data: message.data.unwrap_or_default(),
                        sequence_number: None, // don't inform the application
                        topic: TopicHash::from_raw(message.topic),
                        signature: message.signature, // don't inform the application
                        key: message.key,
                        validated: false,
                    };
                    invalid_messages.push((message, ValidationError::EmptySequenceNumber));
                    continue;
                }
            } else {
                // Do not verify the sequence number, consider it empty
                None
            };

            // Verify the message source if required
            let source = if verify_source {
                if let Some(bytes) = message.from {
                    if !bytes.is_empty() {
                        match PeerId::from_bytes(&bytes) {
                            Ok(peer_id) => Some(peer_id), // valid peer id
                            Err(_) => {
                                // invalid peer id, add to invalid messages
                                tracing::debug!("Message source has an invalid PeerId");
                                let message = RawMessage {
                                    source: None, // don't bother inform the application
                                    data: message.data.unwrap_or_default(),
                                    sequence_number,
                                    topic: TopicHash::from_raw(message.topic),
                                    signature: message.signature, // don't inform the application
                                    key: message.key,
                                    validated: false,
                                };
                                invalid_messages.push((message, ValidationError::InvalidPeerId));
                                continue;
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // This message has passed all validation, add it to the validated messages.
            messages.push(RawMessage {
                source,
                data: message.data.unwrap_or_default(),
                sequence_number,
                topic: TopicHash::from_raw(message.topic),
                signature: message.signature,
                key: message.key,
                validated: false,
            });
        }

        let control = rpc.control.take().unwrap_or_default();
        let mut control_msgs = Vec::new();

        // Collect the gossipsub control messages
        let ihave_msgs: Vec<ControlAction> = control
            .ihave
            .into_iter()
            .map(|ihave| {
                ControlAction::IHave(IHave {
                    topic_hash: TopicHash::from_raw(ihave.topic_id.unwrap_or_default()),
                    message_ids: ihave
                        .message_ids
                        .into_iter()
                        .map(MessageId::from)
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        control_msgs.extend(ihave_msgs);

        let iwant_msgs: Vec<ControlAction> = control
            .iwant
            .into_iter()
            .map(|iwant| {
                ControlAction::IWant(IWant {
                    message_ids: iwant
                        .message_ids
                        .into_iter()
                        .map(MessageId::from)
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        control_msgs.extend(iwant_msgs);

        let graft_msgs: Vec<ControlAction> = control
            .graft
            .into_iter()
            .map(|graft| {
                ControlAction::Graft(Graft {
                    topic_hash: TopicHash::from_raw(graft.topic_id.unwrap_or_default()),
                })
            })
            .collect();
        control_msgs.extend(graft_msgs);

        let mut prune_messages = Vec::new();
        for prune in control.prune {
            // filter out invalid peers
            let peers = prune
                .peers
                .into_iter()
                .filter_map(|info| {
                    info.peer_id
                        .as_ref()
                        .and_then(|id| PeerId::from_bytes(id).ok())
                        .map(|peer_id|
                                    //TODO signedPeerRecord, see https://github.com/libp2p/specs/pull/217
                                    PeerInfo {
                                        peer_id: Some(peer_id),
                                    })
                })
                .collect();
            let topic_hash = TopicHash::from_raw(prune.topic_id.unwrap_or_default());
            prune_messages.push(ControlAction::Prune(Prune {
                topic_hash,
                peers,
                backoff: prune.backoff,
            }));
        }
        control_msgs.extend(prune_messages);

        let mut idontwant_messages = Vec::new();
        for idontwant in control.idontwant {
            idontwant_messages.push(ControlAction::IDontWant(IDontWant {
                message_ids: idontwant
                    .message_ids
                    .into_iter()
                    .map(MessageId::from)
                    .collect::<Vec<_>>(),
            }));
        }
        control_msgs.extend(idontwant_messages);

        let extensions_msg = control.extensions.map(|extensions| Extensions {
            partial_messages: extensions.partial_messages,
        });
        control_msgs.push(ControlAction::Extensions(extensions_msg));

        #[cfg(feature = "partial-messages")]
        let partial_message = rpc.partial.and_then(|partial_proto| {
            let Some(topic_id_bytes) = partial_proto.topic_id else {
                tracing::debug!("Partial message without topic_id, discarding");
                return None;
            };
            let topic_hash = TopicHash::from_raw(String::from_utf8_lossy(&topic_id_bytes));

            let Some(group_id) = partial_proto.group_id else {
                tracing::debug!("Partial message without group_id, discarding");
                return None;
            };

            Some(PartialMessage {
                topic_hash,
                group_id,
                metadata: partial_proto.parts_metadata,
                body: partial_proto.partial_message,
            })
        });

        Ok(Some(HandlerEvent::Message {
            rpc: RpcIn {
                messages,
                subscriptions: rpc
                    .subscriptions
                    .into_iter()
                    .map(|sub| Subscription {
                        action: if Some(true) == sub.subscribe {
                            SubscriptionAction::Subscribe
                        } else {
                            SubscriptionAction::Unsubscribe
                        },
                        topic_hash: TopicHash::from_raw(sub.topic_id.unwrap_or_default()),
                        options: SubscriptionOpts {
                            requests_partial: sub.requests_partial.unwrap_or_default(),
                            supports_partial: sub.supports_partial.unwrap_or_default(),
                        },
                    })
                    .collect(),
                control_msgs,
                #[cfg(feature = "partial-messages")]
                partial_message,
            },
            invalid_messages,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, time::Duration};

    use futures_timer::Delay;
    use libp2p_identity::Keypair;
    use quickcheck::*;

    use super::*;
    use crate::{
        Behaviour, ConfigBuilder, IdentTopic as Topic, MessageAuthenticity, Version,
        config::Config, types::RpcOut,
    };

    #[derive(Clone, Debug)]
    struct Message(RawMessage);

    impl Arbitrary for Message {
        fn arbitrary(g: &mut Gen) -> Self {
            let keypair = TestKeypair::arbitrary(g);

            // generate an arbitrary GossipsubMessage using the behaviour signing functionality
            let config = Config::default();
            let mut gs: Behaviour =
                Behaviour::new(MessageAuthenticity::Signed(keypair.0), config).unwrap();
            let mut data_g = quickcheck::Gen::new(10024);
            let data = (0..u8::arbitrary(&mut data_g))
                .map(|_| u8::arbitrary(g))
                .collect::<Vec<_>>();
            let topic_id = TopicId::arbitrary(g).0;
            Message(gs.build_raw_message(topic_id, data).unwrap())
        }
    }

    #[derive(Clone, Debug)]
    struct TopicId(TopicHash);

    impl Arbitrary for TopicId {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut data_g = quickcheck::Gen::new(1024);
            let topic_string: String = (0..u8::arbitrary(&mut data_g))
                .map(|_| char::arbitrary(g))
                .collect::<String>();
            TopicId(Topic::new(topic_string).into())
        }
    }

    #[derive(Clone)]
    struct TestKeypair(Keypair);

    impl Arbitrary for TestKeypair {
        fn arbitrary(_g: &mut Gen) -> Self {
            // Small enough to be inlined.
            TestKeypair(Keypair::generate_ed25519())
        }
    }

    impl std::fmt::Debug for TestKeypair {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TestKeypair")
                .field("public", &self.0.public())
                .finish()
        }
    }

    #[test]
    /// Test that RPC messages can be encoded and decoded successfully.
    fn encode_decode() {
        fn prop(message: Message) {
            let message = message.0;

            let rpc = RpcOut::Publish {
                message: message.clone(),
                timeout: Delay::new(Duration::from_secs(1)),
                message_id: MessageId(vec![0, 0]),
            };

            let mut codec = GossipsubCodec::new(
                u32::MAX as usize,
                ValidationMode::Strict,
                HashMap::new(),
                5000,
                5000,
            );
            let mut buf = BytesMut::new();
            codec.encode(rpc.into_protobuf(), &mut buf).unwrap();
            let decoded_rpc = codec.decode(&mut buf).unwrap().unwrap();
            // mark as validated as its a published message
            match decoded_rpc {
                HandlerEvent::Message { mut rpc, .. } => {
                    rpc.messages[0].validated = true;

                    assert_eq!(vec![message], rpc.messages);
                }
                _ => panic!("Must decode a message"),
            }
        }

        QuickCheck::new().quickcheck(prop as fn(_) -> _)
    }

    #[test]
    fn support_floodsub_with_custom_protocol() {
        let protocol_config = ConfigBuilder::default()
            .protocol_id("/foosub", Version::V1_1)
            .support_floodsub()
            .build()
            .unwrap()
            .protocol_config();

        assert_eq!(protocol_config.protocol_ids[0].protocol, "/foosub");
        assert_eq!(protocol_config.protocol_ids[1].protocol, "/floodsub/1.0.0");
    }

    #[test]
    fn max_publish_messages() {
        let mut codec = GossipsubCodec::new(
            u32::MAX as usize,
            ValidationMode::Strict,
            HashMap::new(),
            500,
            5000,
        );

        // Create RPC with 501 publish messages (one over limit)
        let rpc = proto::Rpc {
            publish: (0..501).map(|_| proto::Message::default()).collect(),
            ..Default::default()
        };

        let mut buf = BytesMut::new();
        codec.encode(rpc, &mut buf).unwrap();
        let result = codec.decode(&mut buf);

        let err = result.unwrap_err().source().unwrap().to_string();
        assert_eq!(err, "too many publish messages");
    }

    #[test]
    fn max_control_messages() {
        let mut codec = GossipsubCodec::new(
            u32::MAX as usize,
            ValidationMode::Strict,
            HashMap::new(),
            500,
            5000,
        );

        // Create RPC with 501 publish messages (one over limit)
        let rpc = proto::Rpc {
            control: Some(proto::ControlMessage {
                ihave: (0..501).map(|_| proto::ControlIHave::default()).collect(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut buf = BytesMut::new();
        codec.encode(rpc, &mut buf).unwrap();
        let result = codec.decode(&mut buf);

        let err = result.unwrap_err().source().unwrap().to_string();
        assert_eq!(err, "too many control messages");
    }

    #[test]
    fn max_control_message_size_ihave() {
        // Use a small max_control_message_size (100 bytes) to test size limit
        let mut codec = GossipsubCodec::new(
            u32::MAX as usize,
            ValidationMode::Strict,
            HashMap::new(),
            500,
            100, // max_control_message_size: 100 bytes
        );

        // Create RPC with IHAVE containing many message IDs that exceeds 100 bytes
        let rpc = proto::Rpc {
            control: Some(proto::ControlMessage {
                ihave: vec![proto::ControlIHave {
                    topic_id: Some("test-topic".to_string()),
                    message_ids: (0..100).map(|i| vec![i as u8]).collect(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut buf = BytesMut::new();
        codec.encode(rpc, &mut buf).unwrap();
        let result = codec.decode(&mut buf);

        let err = result.unwrap_err().source().unwrap().to_string();
        assert_eq!(err, "rpc control size exceeds max control message size");
    }
    #[test]
    fn max_control_message_size_iwant() {
        // Use a small max_control_message_size (100 bytes) to test size limit
        let mut codec = GossipsubCodec::new(
            u32::MAX as usize,
            ValidationMode::Strict,
            HashMap::new(),
            500,
            100, // max_control_message_size: 100 bytes
        );

        // Create RPC with IWANT containing many message IDs that exceeds 100 bytes
        let rpc = proto::Rpc {
            control: Some(proto::ControlMessage {
                iwant: vec![proto::ControlIWant {
                    message_ids: (0..100).map(|i| vec![i as u8]).collect(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut buf = BytesMut::new();
        codec.encode(rpc, &mut buf).unwrap();
        let result = codec.decode(&mut buf);

        let err = result.unwrap_err().source().unwrap().to_string();
        assert_eq!(err, "rpc control size exceeds max control message size");
    }

    #[test]
    fn max_control_message_size_idontwant() {
        // Use a small max_control_message_size (100 bytes) to test size limit
        let mut codec = GossipsubCodec::new(
            u32::MAX as usize,
            ValidationMode::Strict,
            HashMap::new(),
            500,
            100, // max_control_message_size: 100 bytes
        );

        // Create RPC with IDONTWANT containing many message IDs that exceeds 100 bytes
        let rpc = proto::Rpc {
            control: Some(proto::ControlMessage {
                idontwant: vec![proto::ControlIDontWant {
                    message_ids: (0..100).map(|i| vec![i as u8]).collect(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut buf = BytesMut::new();
        codec.encode(rpc, &mut buf).unwrap();
        let result = codec.decode(&mut buf);

        let err = result.unwrap_err().source().unwrap().to_string();
        assert_eq!(err, "rpc control size exceeds max control message size");
    }

    #[test]
    fn rpc_valid_limits() {
        // Use the default 5KB control message size
        let mut codec = GossipsubCodec::new(
            u32::MAX as usize,
            ValidationMode::Anonymous,
            HashMap::new(),
            500,
            5120, // 5KB max_control_message_size
        );

        // Create RPC with exactly 500 publish messages (at the limit)
        // Use a small IHAVE that fits within 5KB
        let rpc = proto::Rpc {
            publish: (0..500).map(|_| proto::Message::default()).collect(),
            control: Some(proto::ControlMessage {
                ihave: vec![proto::ControlIHave {
                    topic_id: Some("test-topic".to_string()),
                    message_ids: (0..10).map(|i| vec![i as u8]).collect(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut buf = BytesMut::new();
        codec.encode(rpc, &mut buf).unwrap();
        let result = codec.decode(&mut buf);

        // Should succeed
        let event = result.unwrap().expect("Should accept RPC at limits");
        match event {
            HandlerEvent::Message { rpc, .. } => {
                assert_eq!(rpc.messages.len(), 500, "Should have 500 messages");
            }
            _ => panic!("Expected message event"),
        }
    }

    #[test]
    fn max_control_message_size_subscriptions() {
        // Use a small max_control_message_size (100 bytes) to test size limit
        let mut codec = GossipsubCodec::new(
            u32::MAX as usize,
            ValidationMode::Strict,
            HashMap::new(),
            500,
            100, // max_control_message_size: 100 bytes
        );

        // Create RPC with subscriptions containing long topic IDs that exceeds 100 bytes
        let rpc = proto::Rpc {
            subscriptions: (0..10)
                .map(|i| proto::rpc::SubOpts {
                    subscribe: Some(true),
                    topic_id: Some(format!("very-long-topic-name-{}", i)),
                    requests_partial: None,
                    supports_partial: None,
                })
                .collect(),
            ..Default::default()
        };

        let mut buf = BytesMut::new();
        codec.encode(rpc, &mut buf).unwrap();
        let result = codec.decode(&mut buf);

        // This should fail because subscriptions exceed the max_control_message_size
        // Currently this test will FAIL because subscriptions are not yet validated
        let err = result.unwrap_err().source().unwrap().to_string();
        assert!(
            err.contains("rpc control size exceeds max control message size"),
            "Expected error about subscription size, got: {}",
            err
        );
    }
}
