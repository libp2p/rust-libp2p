use std::borrow::Cow;
use std::iter;

use async_trait::async_trait;
use multiaddr::Multiaddr;
use libp2p::bytes::Bytes;
use libp2p::gossipsub::{Gossipsub, GossipsubEvent, IdentTopic, MessageAuthenticity};
use libp2p::identify;
use libp2p::identity::Keypair;
use libp2p::kad::store::MemoryStore;
use libp2p::kad::{Kademlia, KademliaConfig, KademliaEvent};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{
    ProtocolSupport, RequestResponse, RequestResponseEvent, RequestResponseMessage,
};
use libp2p::swarm::{
    ConnectionError, ConnectionHandler, IntoConnectionHandler,
    NetworkBehaviour, SwarmEvent,
};
use libp2p::PeerId;
use libp2p::gossipsub;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::mychat_direct_message_protocol::{
    MyChatMessageExchangeCodec, MyChatMessageExchangeProtocol, MyChatMessageRequest,
    MyChatMessageResponse,
};
use crate::mychat_behaviour::MyChatNetworkBehaviourError::{
    BootstrapError, InvalidBootstrapPeerAddress, PubSubBehaviourError,
};
use crate::mychat_direct_message_protocol;
use crate::network::{Address, Instruction, NetworkError, Notification};
use crate::network::event_handler::EventHandler;
use crate::network::instruction_handler::InstructionHandler;
use crate::network::NetworkError::SendError;

const BROADCAST_TOPIC: &str = "BROADCAST";
const KADEMLIA_PROTO_NAME: &[u8] = b"/my-chat/kad/1.0.0";
const IDENTIFY_PROTO_NAME: &str = "/my-chat/ide/1.0.0";
const GOSSIPSUB_PROTO_ID_PREFIX: &str = "my-chat/gossipsub";

/// This behaviour composes multiple behaviours. It uses the
/// Identify and Kademlia behaviours for peer discovery.
/// For the chat functionality, it uses Gossipsub and
/// RequestResponse behaviours.
///
/// - `identify::Behaviour`: when peers connect and they
///   both support this protocol, they exchange `IdentityInfo`.
///   `MyChatNetworkBehaviour` then uses this info to add their
///   listen addresses to the Kademlia DHT.
///   Without `identify::Behaviour`, the DHT would propagate
///   the peer ids of peers, but not their listen addresses,
///   making it impossible for peers to connect to them.
/// - `Kademlia`: this is the Distributed Hash Table implementation,
///   primarily used to distribute info about other peers on the
///   network this peer knows about to connected peers. This is a core
///   feature of `Kademlia` that triggers automatically.
///   This enables so-called DHT-routing, which enables peers to send
///   messages to peers they are not directly connected to.
/// - `Gossipsub`: this takes care of sending pubsub messages to all
///   peers this peer is aware of on a certain topic.
///   Subscribe/unsubscribe messages are also propagated.
///   This works well in combination with `identify::Behaviour`
///   and `Kademlia`, because they ensure that Gossipsub messages
///   not only reach directly connected peers, but all peers that can
///   be reached through the DHT routing.
/// - `RequestResponse`: `MyChatMessageExchangeCodec` is a simple
///   request response protocol, where received messages will
///   automatically be responded to with a 0 byte. This confirms
///   to the sender that the message was correctly received.
///   Consider that analogous to the 'delivered' check on WhatsApp/Signal.
///   This can only be used to send message to directly connected peers.
///   It does not use the DHT routing. It expect the local peer to know
///   the listen address of the peer it is sending a message to.
///
///   If we expect peers to not always be able to directly reach
///   other peers because of networking reasons, we may have
///   to add the `Relay` behaviour to `MyChatNetworkBehaviour`
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "MyChatNetworkBehaviourEvent")]
pub struct MyChatNetworkBehaviour {
    /// The Gossipsub pub/sub behaviour is used to send broadcast messages to peers.
    pub pubsub: Gossipsub,
    /// Send more detailed identifying info to connected peers, a.o the listen_address.
    /// This address can then be used to populate the Kademlia DHT.
    pub identify: identify::Behaviour,
    /// The Kademlia DHT used to discover peers
    pub kademlia: Kademlia<MemoryStore>,
    /// In this case, RequestResponse behaviour is used to send direct messages to peers,
    /// with delivery confirmation. This means that as long as the network does not return an error to its caller,
    /// direct messages can be assumed to have been delivered.
    pub request_response: RequestResponse<MyChatMessageExchangeCodec>,
}

impl MyChatNetworkBehaviour {
    pub fn new(
        keypair: &Keypair,
        bootstrap_peers: Option<Vec<Multiaddr>>,
    ) -> Result<MyChatNetworkBehaviour, MyChatNetworkBehaviourError> {
        let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
            .protocol_id_prefix(GOSSIPSUB_PROTO_ID_PREFIX)
            .build()
            .map_err(|err| PubSubBehaviourError(err.to_string()))?;

        let mut pubsub = Gossipsub::new(
            MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|err| PubSubBehaviourError(err.to_string()))?;

        pubsub
            .subscribe(&IdentTopic::new(BROADCAST_TOPIC))
            .map_err(|err| PubSubBehaviourError(err.to_string()))?;

        let identify = identify::Behaviour::new(identify::Config::new(
            IDENTIFY_PROTO_NAME.to_string(),
            keypair.public(),
        ));

        let peer_id = keypair.public().to_peer_id();
        let mut kademlia = Kademlia::with_config(
            peer_id,
            MemoryStore::new(peer_id),
            KademliaConfig::default()
                .set_protocol_names(iter::once(Cow::Borrowed(KADEMLIA_PROTO_NAME)).collect())
                .to_owned(),
        );

        if let Some(bootstrap_peers) = bootstrap_peers {
            // First, we add the addresses of the bootstrap nodes to our view of the DHT
            for peer_address in &bootstrap_peers {
                let peer_id = extract_peer_id_from_multiaddr(peer_address)?;
                kademlia.add_address(&peer_id, peer_address.clone());
            }

            // Next, we add our own info to the DHT. This will then automatically be shared
            // with the other peers on the DHT. This operation will fail if we are a bootstrap peer.
            kademlia
                .bootstrap()
                .map_err(|err| BootstrapError(err.to_string()))?;
        }

        Ok(Self {
            pubsub,
            identify,
            kademlia,
            request_response: RequestResponse::new(
                MyChatMessageExchangeCodec(),
                iter::once((MyChatMessageExchangeProtocol(), ProtocolSupport::Full)),
                Default::default(),
            ),
        })
    }

    fn notify(notification_tx: &mpsc::UnboundedSender<Notification>, notification: Notification) {
        if let Err(e) = notification_tx.send(notification) {
            log::error!("Failed to send notification back to router through mpsc channel: {e}");
        }
    }

    fn notify_error(notification_tx: &mpsc::UnboundedSender<Notification>, error: NetworkError) {
        if let Err(e) = notification_tx.send(Notification::Err(error)) {
            log::error!("Failed to send notification back to router through mpsc channel: {e}");
        }
    }

    fn send(&mut self, destination: Address<PeerId>, message: Bytes) -> Result<(), NetworkError> {
        match destination {
            Address::DirectMessage(peer_id) => self.send_single(&peer_id, &message)?,
            Address::Broadcast => {
                log::debug!("Broadcasting: {message:?}");
                if let Err(error) = self
                    .pubsub
                    .publish(IdentTopic::new(BROADCAST_TOPIC), message)
                {
                    log::error!(
                        "Failed to publish message to the pubsub topic '{}': {}",
                        BROADCAST_TOPIC,
                        error
                    );
                    return Err(NetworkError::BroadcastError {
                        reason: format!("{error}"),
                    });
                }
            }
        }
        Ok(())
    }

    fn send_single(&mut self, peer_id: &PeerId, message: &Bytes) -> Result<(), NetworkError> {
        let request_id = self
            .request_response
            .send_request(peer_id, MyChatMessageRequest(message.clone()));

        log::debug!(
            "Sending message to peer {} with id {}: {:?}",
            peer_id,
            request_id,
            message
        );

        Ok(())
    }

    fn handle_request_response_message(
        &mut self,
        notification_tx: &mpsc::UnboundedSender<Notification>,
        peer_id: PeerId,
        message: RequestResponseMessage<MyChatMessageRequest, MyChatMessageResponse>,
    ) {
        match message {
            RequestResponseMessage::Request {
                request,
                channel,
                request_id,
            } => {
                log::debug!(
                    "Received direct message request with id {} from peer {}: {:?}",
                    request_id,
                    peer_id,
                    request.0
                );

                Self::notify(notification_tx, Notification::Data(request.0));

                log::debug!(
                    "Acknowledging reception of message with id {request_id} to requesting peer."
                );
                if self
                    .request_response
                    .send_response(
                        channel,
                        MyChatMessageResponse(Bytes::from_static(
                            mychat_direct_message_protocol::ACK_MESSAGE,
                        )),
                    )
                    .is_err()
                {
                    log::error!("Failed to acknowledge reception of message with id {request_id} to requesting peer")
                }
            }
            RequestResponseMessage::Response {
                response,
                request_id,
            } => {
                if response.0 == Bytes::from_static(mychat_direct_message_protocol::ACK_MESSAGE) {
                    log::info!(
                        "Received message reception confirmation from peer {} for request {}",
                        peer_id,
                        request_id
                    );
                } else {
                    log::error!(
                        "Unexpected payload for message reception confirmation from peer {} for request {}: {:?}",
                        peer_id,
                        request_id,
                        response.0
                    )
                }
            }
        }
    }

    fn handle_pubsub_event(
        &mut self,
        notification_tx: &mpsc::UnboundedSender<Notification>,
        event: GossipsubEvent,
    ) {
        match event {
            GossipsubEvent::Message { message, .. } => {
                Self::notify(notification_tx, Notification::Data(message.data.into()));
            }
            _ => log::debug!("Received Pubsub event:  {event:?}"),
        }
    }

    fn handle_request_response_event(
        &mut self,
        notification_tx: &mpsc::UnboundedSender<Notification>,
        event: RequestResponseEvent<MyChatMessageRequest, MyChatMessageResponse>,
    ) {
        match event {
            RequestResponseEvent::Message {
                peer: peer_id,
                message,
            } => self.handle_request_response_message(notification_tx, peer_id, message),
            RequestResponseEvent::OutboundFailure {
                peer: peer_id,
                error,
                request_id,
            } => {
                Self::notify_error(
                    notification_tx,
                    SendError {
                        peer_id,
                        reason: format!("Failed sending request with id '{request_id}': {error}"),
                    },
                );
            }
            RequestResponseEvent::InboundFailure {
                peer: peer_id,
                error,
                request_id,
            } => {
                Self::notify_error(
                    notification_tx,
                    NetworkError::ReceiveError {
                        peer_id,
                        reason: format!(
                            "Could not handle incoming request with id '{request_id}': {error}"
                        ),
                    },
                );
            }
            _ => log::trace!("Unhandled RequestResponseEvent event: {event:?}"),
        }
    }

    /// When we receive IdentityInfo, if the peer supports our Kademlia protocol, we add
    /// their listen addresses to the DHT, so they will be propagated to other peers.
    fn handle_identify_event(&mut self, identify_event: Box<identify::Event>) {
        log::debug!("Received identify::Event: {:?}", *identify_event);

        if let identify::Event::Received {
            peer_id,
            info:
                identify::Info {
                    listen_addrs,
                    protocols,
                    ..
                },
        } = *identify_event
        {
            if protocols
                .iter()
                .any(|p| p.as_bytes() == KADEMLIA_PROTO_NAME)
            {
                for addr in listen_addrs {
                    log::debug!("Adding received IdentifyInfo matching protocol '{}' to the DHT. Peer: {}, addr: {}", String::from_utf8_lossy(KADEMLIA_PROTO_NAME), peer_id, addr);
                    self.kademlia.add_address(&peer_id, addr);
                }
            }
        }
    }

    fn handle_non_functional_event(
        &mut self,
        event: SwarmEvent<
            <MyChatNetworkBehaviour as NetworkBehaviour>::OutEvent,
            <<<MyChatNetworkBehaviour as NetworkBehaviour>::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::Error,
        >,
    ) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                log::info!("Listening on {address:?}")
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error } => match peer_id {
                None => log::error!("Could not connect: {}", error),
                Some(peer_id) => log::error!("Could not connect to peer '{}': {}", peer_id, error),
            },
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause: Some(ConnectionError::Handler(error)),
                ..
            } => {
                log::error!(
                    "Connection to peer {} closed because of an error: {}",
                    peer_id,
                    error
                );
            }
            _ => log::trace!("Unhandled Swarm event: {event:?}"),
        }
    }
}

fn extract_peer_id_from_multiaddr(
    address_with_peer_id: &Multiaddr,
) -> Result<PeerId, MyChatNetworkBehaviourError> {
    match address_with_peer_id.iter().last() {
        Some(Protocol::P2p(hash)) => PeerId::from_multihash(hash).map_err(|multihash| {
            InvalidBootstrapPeerAddress(format!(
                "Invalid PeerId '{multihash:?}' in Multiaddr '{address_with_peer_id}'"
            ))
        }),
        _ => Err(InvalidBootstrapPeerAddress(
            "Multiaddr does not contain peer_id".to_string(),
        )),
    }
}

#[async_trait]
impl InstructionHandler for MyChatNetworkBehaviour {
    async fn handle_instruction(
        &mut self,
        notification_tx: &mpsc::UnboundedSender<Notification>,
        instruction: Instruction,
    ) {
        match instruction {
            Instruction::Send {
                destination,
                message,
            } => {
                if let Err(error) = self.send(destination, message) {
                    Self::notify(notification_tx, Notification::Err(error));
                }
            }
            Instruction::PeerList => {
                Self::notify(
                    notification_tx,
                    Notification::PeerList(self.pubsub.all_peers().map(|peer| *peer.0).collect()),
                );
            }
        }
    }
}
#[async_trait]
impl EventHandler for MyChatNetworkBehaviour {
    async fn handle_event(
        &mut self,
        notification_tx: &mpsc::UnboundedSender<Notification>,
        event: SwarmEvent<
            <MyChatNetworkBehaviour as NetworkBehaviour>::OutEvent,
            <<<MyChatNetworkBehaviour as NetworkBehaviour>::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::Error
        >,
    ) {
        match event {
            SwarmEvent::Behaviour(MyChatNetworkBehaviourEvent::Pubsub(event)) => {
                self.handle_pubsub_event(notification_tx, event)
            }
            SwarmEvent::Behaviour(MyChatNetworkBehaviourEvent::RequestResponse(event)) => {
                self.handle_request_response_event(notification_tx, event)
            }
            SwarmEvent::Behaviour(MyChatNetworkBehaviourEvent::IdentifyEvent(e)) => {
                self.handle_identify_event(e)
            }
            non_functional_event => self.handle_non_functional_event(non_functional_event),
        }
    }
}

#[derive(Debug)]
pub enum MyChatNetworkBehaviourEvent {
    Pubsub(GossipsubEvent),
    KademliaEvent(Box<KademliaEvent>),
    IdentifyEvent(Box<identify::Event>),
    RequestResponse(RequestResponseEvent<MyChatMessageRequest, MyChatMessageResponse>),
}

impl From<GossipsubEvent> for MyChatNetworkBehaviourEvent {
    fn from(event: GossipsubEvent) -> Self {
        MyChatNetworkBehaviourEvent::Pubsub(event)
    }
}

impl From<KademliaEvent> for MyChatNetworkBehaviourEvent {
    fn from(event: KademliaEvent) -> Self {
        MyChatNetworkBehaviourEvent::KademliaEvent(Box::new(event))
    }
}

impl From<identify::Event> for MyChatNetworkBehaviourEvent {
    fn from(event: identify::Event) -> Self {
        MyChatNetworkBehaviourEvent::IdentifyEvent(Box::new(event))
    }
}

impl From<RequestResponseEvent<MyChatMessageRequest, MyChatMessageResponse>>
    for MyChatNetworkBehaviourEvent
{
    fn from(event: RequestResponseEvent<MyChatMessageRequest, MyChatMessageResponse>) -> Self {
        MyChatNetworkBehaviourEvent::RequestResponse(event)
    }
}

#[derive(Debug, Error)]
pub enum MyChatNetworkBehaviourError {
    // This error is deliberately generic, because we don't want to break the error API
    // when we change the pubsub behaviour.
    #[error("Could not construct composed pubsub behaviour: {0}")]
    PubSubBehaviourError(String),
    #[error("Failed bootstrap with the DHT: {0}")]
    BootstrapError(String),
    #[error("The address for the bootstrap peer is invalid: {0}")]
    InvalidBootstrapPeerAddress(String),
}
