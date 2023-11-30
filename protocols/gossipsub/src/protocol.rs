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

use crate::config::ValidationMode;
use crate::handler::HandlerEvent;
use crate::rpc_proto::proto;
use crate::topic::TopicHash;
use crate::types::{
    ControlAction, MessageId, PeerInfo, PeerKind, RawMessage, Rpc, Subscription, SubscriptionAction,
};
use crate::ValidationError;
use asynchronous_codec::{Decoder, Encoder, Framed};
use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use futures::future;
use futures::prelude::*;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_identity::{PeerId, PublicKey};
use libp2p_swarm::StreamProtocol;
use quick_protobuf::Writer;
use std::pin::Pin;
use void::Void;

pub(crate) const SIGNING_PREFIX: &[u8] = b"libp2p-pubsub:";

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
    /// The maximum transmit size for a packet.
    pub(crate) max_transmit_size: usize,
    /// Determines the level of validation to be done on incoming messages.
    pub(crate) validation_mode: ValidationMode,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            max_transmit_size: 65536,
            validation_mode: ValidationMode::Strict,
            protocol_ids: vec![GOSSIPSUB_1_1_0_PROTOCOL, GOSSIPSUB_1_0_0_PROTOCOL],
        }
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
    type Error = Void;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: TSocket, protocol_id: Self::Info) -> Self::Future {
        Box::pin(future::ok((
            Framed::new(
                socket,
                GossipsubCodec::new(self.max_transmit_size, self.validation_mode),
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
    type Error = Void;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: TSocket, protocol_id: Self::Info) -> Self::Future {
        Box::pin(future::ok((
            Framed::new(
                socket,
                GossipsubCodec::new(self.max_transmit_size, self.validation_mode),
            ),
            protocol_id.kind,
        )))
    }
}

/* Gossip codec for the framing */

pub struct GossipsubCodec {
    /// Determines the level of validation performed on incoming messages.
    validation_mode: ValidationMode,
    /// The codec to handle common encoding/decoding of protobuf messages
    codec: quick_protobuf_codec::Codec<proto::RPC>,
}

impl GossipsubCodec {
    pub fn new(max_length: usize, validation_mode: ValidationMode) -> GossipsubCodec {
        let codec = quick_protobuf_codec::Codec::new(max_length);
        GossipsubCodec {
            validation_mode,
            codec,
        }
    }

    /// Verifies a gossipsub message. This returns either a success or failure. All errors
    /// are logged, which prevents error handling in the codec and handler. We simply drop invalid
    /// messages and log warnings, rather than propagating errors through the codec.
    fn verify_signature(message: &proto::Message) -> bool {
        use quick_protobuf::MessageWrite;

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
        let mut buf = Vec::with_capacity(message_sig.get_size());
        let mut writer = Writer::new(&mut buf);
        message_sig
            .write_message(&mut writer)
            .expect("Encoding to succeed");
        let mut signature_bytes = SIGNING_PREFIX.to_vec();
        signature_bytes.extend_from_slice(&buf);
        public_key.verify(&signature_bytes, signature)
    }
}

impl Encoder for GossipsubCodec {
    type Item<'a> = proto::RPC;
    type Error = quick_protobuf_codec::Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.codec.encode(item, dst)
    }
}

impl Decoder for GossipsubCodec {
    type Item = HandlerEvent;
    type Error = quick_protobuf_codec::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let Some(rpc) = self.codec.decode(src)? else {
            return Ok(None);
        };
        // Store valid messages.
        let mut messages = Vec::with_capacity(rpc.publish.len());
        // Store any invalid messages.
        let mut invalid_messages = Vec::new();

        for message in rpc.publish.into_iter() {
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
                        tracing::warn!("Message dropped. Message source was non-empty and anonymous validation mode is set");
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

        let mut control_msgs = Vec::new();

        if let Some(rpc_control) = rpc.control {
            // Collect the gossipsub control messages
            let ihave_msgs: Vec<ControlAction> = rpc_control
                .ihave
                .into_iter()
                .map(|ihave| ControlAction::IHave {
                    topic_hash: TopicHash::from_raw(ihave.topic_id.unwrap_or_default()),
                    message_ids: ihave
                        .message_ids
                        .into_iter()
                        .map(MessageId::from)
                        .collect::<Vec<_>>(),
                })
                .collect();

            let iwant_msgs: Vec<ControlAction> = rpc_control
                .iwant
                .into_iter()
                .map(|iwant| ControlAction::IWant {
                    message_ids: iwant
                        .message_ids
                        .into_iter()
                        .map(MessageId::from)
                        .collect::<Vec<_>>(),
                })
                .collect();

            let graft_msgs: Vec<ControlAction> = rpc_control
                .graft
                .into_iter()
                .map(|graft| ControlAction::Graft {
                    topic_hash: TopicHash::from_raw(graft.topic_id.unwrap_or_default()),
                })
                .collect();

            let mut prune_msgs = Vec::new();

            for prune in rpc_control.prune {
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
                    .collect::<Vec<PeerInfo>>();

                let topic_hash = TopicHash::from_raw(prune.topic_id.unwrap_or_default());
                prune_msgs.push(ControlAction::Prune {
                    topic_hash,
                    peers,
                    backoff: prune.backoff,
                });
            }

            control_msgs.extend(ihave_msgs);
            control_msgs.extend(iwant_msgs);
            control_msgs.extend(graft_msgs);
            control_msgs.extend(prune_msgs);
        }

        Ok(Some(HandlerEvent::Message {
            rpc: Rpc {
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
                    })
                    .collect(),
                control_msgs,
            },
            invalid_messages,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::{Behaviour, ConfigBuilder};
    use crate::{IdentTopic as Topic, Version};
    use libp2p_identity::Keypair;
    use quickcheck::*;

    #[derive(Clone, Debug)]
    struct Message(RawMessage);

    impl Arbitrary for Message {
        fn arbitrary(g: &mut Gen) -> Self {
            let keypair = TestKeypair::arbitrary(g);

            // generate an arbitrary GossipsubMessage using the behaviour signing functionality
            let config = Config::default();
            let mut gs: Behaviour =
                Behaviour::new(crate::MessageAuthenticity::Signed(keypair.0), config).unwrap();
            let data = (0..g.gen_range(10..10024u32))
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
            let topic_string: String = (0..g.gen_range(20..1024u32))
                .map(|_| char::arbitrary(g))
                .collect::<String>();
            TopicId(Topic::new(topic_string).into())
        }
    }

    #[derive(Clone)]
    struct TestKeypair(Keypair);

    impl Arbitrary for TestKeypair {
        #[cfg(feature = "rsa")]
        fn arbitrary(g: &mut Gen) -> Self {
            let keypair = if bool::arbitrary(g) {
                // Small enough to be inlined.
                Keypair::generate_ed25519()
            } else {
                // Too large to be inlined.
                let mut rsa_key = hex::decode("308204bd020100300d06092a864886f70d0101010500048204a7308204a30201000282010100ef930f41a71288b643c1cbecbf5f72ab53992249e2b00835bf07390b6745419f3848cbcc5b030faa127bc88cdcda1c1d6f3ff699f0524c15ab9d2c9d8015f5d4bd09881069aad4e9f91b8b0d2964d215cdbbae83ddd31a7622a8228acee07079f6e501aea95508fa26c6122816ef7b00ac526d422bd12aed347c37fff6c1c307f3ba57bb28a7f28609e0bdcc839da4eedca39f5d2fa855ba4b0f9c763e9764937db929a1839054642175312a3de2d3405c9d27bdf6505ef471ce85c5e015eee85bf7874b3d512f715de58d0794fd8afe021c197fbd385bb88a930342fac8da31c27166e2edab00fa55dc1c3814448ba38363077f4e8fe2bdea1c081f85f1aa6f02030100010282010028ff427a1aac1a470e7b4879601a6656193d3857ea79f33db74df61e14730e92bf9ffd78200efb0c40937c3356cbe049cd32e5f15be5c96d5febcaa9bd3484d7fded76a25062d282a3856a1b3b7d2c525cdd8434beae147628e21adf241dd64198d5819f310d033743915ba40ea0b6acdbd0533022ad6daa1ff42de51885f9e8bab2306c6ef1181902d1cd7709006eba1ab0587842b724e0519f295c24f6d848907f772ae9a0953fc931f4af16a07df450fb8bfa94572562437056613647818c238a6ff3f606cffa0533e4b8755da33418dfbc64a85110b1a036623c947400a536bb8df65e5ebe46f2dfd0cfc86e7aeeddd7574c253e8fbf755562b3669525d902818100f9fff30c6677b78dd31ec7a634361438457e80be7a7faf390903067ea8355faa78a1204a82b6e99cb7d9058d23c1ecf6cfe4a900137a00cecc0113fd68c5931602980267ea9a95d182d48ba0a6b4d5dd32fdac685cb2e5d8b42509b2eb59c9579ea6a67ccc7547427e2bd1fb1f23b0ccb4dd6ba7d206c8dd93253d70a451701302818100f5530dfef678d73ce6a401ae47043af10a2e3f224c71ae933035ecd68ccbc4df52d72bc6ca2b17e8faf3e548b483a2506c0369ab80df3b137b54d53fac98f95547c2bc245b416e650ce617e0d29db36066f1335a9ba02ad3e0edf9dc3d58fd835835042663edebce81803972696c789012847cb1f854ab2ac0a1bd3867ac7fb502818029c53010d456105f2bf52a9a8482bca2224a5eac74bf3cc1a4d5d291fafcdffd15a6a6448cce8efdd661f6617ca5fc37c8c885cc3374e109ac6049bcbf72b37eabf44602a2da2d4a1237fd145c863e6d75059976de762d9d258c42b0984e2a2befa01c95217c3ee9c736ff209c355466ff99375194eff943bc402ea1d172a1ed02818027175bf493bbbfb8719c12b47d967bf9eac061c90a5b5711172e9095c38bb8cc493c063abffe4bea110b0a2f22ac9311b3947ba31b7ef6bfecf8209eebd6d86c316a2366bbafda7279b2b47d5bb24b6202254f249205dcad347b574433f6593733b806f84316276c1990a016ce1bbdbe5f650325acc7791aefe515ecc60063bd02818100b6a2077f4adcf15a17092d9c4a346d6022ac48f3861b73cf714f84c440a07419a7ce75a73b9cbff4597c53c128bf81e87b272d70428a272d99f90cd9b9ea1033298e108f919c6477400145a102df3fb5601ffc4588203cf710002517bfa24e6ad32f4d09c6b1a995fa28a3104131bedd9072f3b4fb4a5c2056232643d310453f").unwrap();
                Keypair::rsa_from_pkcs8(&mut rsa_key).unwrap()
            };
            TestKeypair(keypair)
        }

        #[cfg(not(feature = "rsa"))]
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

            let rpc = Rpc {
                messages: vec![message.clone()],
                subscriptions: vec![],
                control_msgs: vec![],
            };

            let mut codec = GossipsubCodec::new(u32::MAX as usize, ValidationMode::Strict);
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
}
