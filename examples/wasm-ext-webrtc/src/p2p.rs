use anyhow::Result;
use async_channel as channel;
use futures::future::Either;
use futures::StreamExt;
use libp2p::core::transport::OrTransport;
use libp2p::core::upgrade;
use libp2p::core::Transport;
use libp2p::core::{muxing::StreamMuxerBox, transport::Boxed};
use libp2p::identity;
use libp2p::noise;
use libp2p::swarm::{keep_alive, NetworkBehaviour};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::wasm_ext::{ffi, ExtTransport};
use libp2p::yamux;
use libp2p::Swarm;
use libp2p::{Multiaddr, PeerId};
use libp2p_gossipsub as gossipsub;
use libp2p_gossipsub::IdentTopic;
use std::collections::VecDeque;
use wasm_bindgen_futures::spawn_local;

enum Command {
    Dial(Multiaddr),
    Chat(String),
}

enum Event {
    Connected(PeerId),
    Disconnected(PeerId),
    Message(String),
    Error(String),
}

pub struct P2pApp {
    event_rx: channel::Receiver<Event>,
    command_tx: channel::Sender<Command>,
    messages: VecDeque<String>,
    connected: bool,
    text: String,
}

impl P2pApp {
    pub fn new() -> Self {
        let (event_tx, event_rx) = channel::bounded(64);
        let (command_tx, command_rx) = channel::bounded(64);

        // Start libp2p network service.
        // spawn_local(network_service(command_rx, event_tx));

        Self {
            event_rx,
            command_tx,
            messages: VecDeque::new(),
            connected: false,
            text: String::new(),
        }
    }

    fn send_command(&self, command: Command) {
        let tx = self.command_tx.clone();
        spawn_local(async move {
            let _ = tx.send(command).await;
        });
    }

    fn send_chat(&mut self) {
        self.send_command(Command::Chat(self.text.clone()));
        self.messages.push_back(format!("{: >20}", self.text));
        self.text.clear();
    }

    pub async fn start(&self) -> Result<()> {
        // ...
        Ok(())
    }
}

async fn network_service(
    mut command_rx: channel::Receiver<Command>,
    event_tx: channel::Sender<Event>,
) {
    // Create the transport.

    let local_key = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(local_key.public());

    let authentication_config = noise::Config::new(&local_key).unwrap();
    let mut yamux_config = yamux::Config::default();

    let webrtc = ExtTransport::new(ffi::webrtc_transport(
        local_key.to_protobuf_encoding().unwrap(),
    ));

    let webrtc_direct = ExtTransport::new(ffi::webrtc_direct_transport(
        local_key.to_protobuf_encoding().unwrap(),
    ))
    .upgrade(upgrade::Version::V1)
    .authenticate(authentication_config)
    .multiplex(yamux_config);

    let transport = webrtc
        .or_transport(webrtc_direct)
        .map(|either_output, _| match either_output {
            Either::Left(conn) => (peer_id, StreamMuxerBox::new(conn)),
            Either::Right(conn) => (peer_id, StreamMuxerBox::new(conn)),
        })
        .boxed();

    #[derive(NetworkBehaviour)]
    struct Behaviour {
        keep_alive: keep_alive::Behaviour,
        gossipsub: gossipsub::Behaviour,
    }

    let topic = gossipsub::IdentTopic::new("chat");

    // Create a Swarm to manage peers and events
    let mut swarm = {
        let local_peer_id = PeerId::from(local_key.public());
        let mut behaviour = Behaviour {
            gossipsub: gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(local_key),
                gossipsub::ConfigBuilder::default()
                    .build()
                    .expect("Valid config"),
            )
            .expect("Correct configuration"),
            keep_alive: keep_alive::Behaviour::default(),
        };

        behaviour.gossipsub.subscribe(&topic.clone());

        SwarmBuilder::with_wasm_executor(transport, behaviour, local_peer_id).build()
    };

    // Manage Swarm events and UI channels.
    loop {
        futures::select! {
            command = command_rx.select_next_some() => match command {
                Command::Dial(addr) => {
                    if let Err(e) = swarm.dial(addr) {
                        let _ = event_tx.send(Event::Error(e.to_string())).await;
                    }
                }
                Command::Chat(message) => {
                    swarm
                        .behaviour_mut()
                        .gossipsub
                        .publish(topic.clone(), message.as_bytes());
                }
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(gossipsub::Event::Message

                 {
                    message_id: _,
                    propagation_source: _,
                    message,
                }

                )) => {
                    let event = Event::Message(String::from_utf8_lossy(&message.data).into());
                    let _ = event_tx.send(event).await;
                },
                // SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                //     swarm
                //         .behaviour_mut()
                //         .floodsub
                //         .add_node_to_partial_view(peer_id);
                //     let _ = event_tx.send(Event::Connected(peer_id)).await;
                // }
                // SwarmEvent::ConnectionClosed { peer_id, .. } => {
                //     swarm
                //         .behaviour_mut()
                //         .floodsub
                //         .remove_node_from_partial_view(&peer_id);
                //     let _ = event_tx.send(Event::Disconnected(peer_id)).await;
                // }
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    let _ = event_tx.send(Event::Error(error.to_string())).await;
                }
                event => super::console_log!("Swarm event: {event:?}"),
            }
        }
    }
}
