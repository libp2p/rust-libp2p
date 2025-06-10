use std::str::FromStr;

use futures::{channel::mpsc, StreamExt};
use js_sys::{Object, Reflect};
use libp2p::{
    identify, identity::Keypair, multiaddr::Protocol, noise, ping, swarm::SwarmEvent, yamux,
    Multiaddr, Swarm, Transport,
};
use libp2p_core::{muxing::StreamMuxerBox, upgrade::Version};
use libp2p_swarm::NetworkBehaviour;
use libp2p_webrtc_websys::browser::{self, Behaviour, Transport as BrowserWebrtcTransport};
use tracing;
use tracing_wasm;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn initialize() {
    tracing_wasm::set_as_global_default_with_config(
        tracing_wasm::WASMLayerConfigBuilder::new()
            .set_max_level(tracing::Level::INFO)
            .set_console_config(tracing_wasm::ConsoleConfig::ReportWithConsoleColor)
            .build(),
    );
}

#[wasm_bindgen]
pub struct BrowserTransport {
    cmd_sender: mpsc::UnboundedSender<Command>,
    event_receiver: std::sync::Arc<futures::lock::Mutex<mpsc::UnboundedReceiver<Event>>>,
    peer_id: String,
}

enum Command {
    ListenOnRelay { addr: Multiaddr },
    DialPeer { addr: Multiaddr },
}

#[derive(Debug, Clone)]
enum Event {
    ReservationCreated,
    ConnectionEstablished { peer_id: String },
    PingSuccess { peer_id: String, rtt_ms: f64 },
    Error { msg: String },
}

#[derive(NetworkBehaviour)]
pub struct WebRTCBehaviour {
    relay: libp2p_relay::client::Behaviour,
    webrtc: Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
}

#[wasm_bindgen]
impl BrowserTransport {
    #[wasm_bindgen]
    pub fn new() -> Result<BrowserTransport, JsError> {
        let keypair = Keypair::generate_ed25519();
        let local_peer_id = keypair.public().to_peer_id();

        let (relay_transport, relay_behaviour) = libp2p_relay::client::new(local_peer_id);

        let relay_transport_upgraded = relay_transport
            .upgrade(Version::V1)
            .authenticate(noise::Config::new(&keypair)?)
            .multiplex(yamux::Config::default())
            .boxed();

        let ws_transport = libp2p_websocket_websys::Transport::default()
            .upgrade(Version::V1)
            .authenticate(noise::Config::new(&keypair)?)
            .multiplex(yamux::Config::default())
            .boxed();

        let webrtc_config = libp2p_webrtc_websys::browser::Config {
            keypair: keypair.clone(),
        };
        let (webrtc_transport, webrtc_behaviour) = BrowserWebrtcTransport::new(webrtc_config);

        let webrtc_transport_boxed = webrtc_transport.boxed();

        let behaviour = WebRTCBehaviour {
            relay: relay_behaviour,
            webrtc: webrtc_behaviour,
            identify: identify::Behaviour::new(identify::Config::new(
                "browser-to-browser-webrtc/1.0.0".into(),
                keypair.public(),
            )),
            ping: ping::Behaviour::new(ping::Config::default()),
        };

        let final_transport =
            webrtc_transport_boxed
                .or_transport(relay_transport_upgraded)
                .or_transport(ws_transport)
                .map(|either_output, _| {
                    match either_output {
                        futures::future::Either::Left(futures::future::Either::Left((
                            peer_id,
                            connection,
                        ))) => (peer_id, StreamMuxerBox::new(connection)),
                        futures::future::Either::Left(futures::future::Either::Right(output)) => {
                            output
                        } 
                        futures::future::Either::Right(output) => output, 
                    }
                })
                .boxed();

        let mut swarm = Swarm::new(
            final_transport,
            behaviour,
            local_peer_id,
            libp2p::swarm::Config::with_executor(Box::new(|fut| {
                wasm_bindgen_futures::spawn_local(fut);
            })),
        );

        let (cmd_sender, mut cmd_receiver) = mpsc::unbounded();
        let (event_sender, event_receiver) = mpsc::unbounded();

        spawn_local(async move {
            loop {
                futures::select! {
                    cmd = cmd_receiver.next() => {
                        if let Some(cmd) = cmd {
                            match cmd {
                                Command::ListenOnRelay { addr } => {
                                    // Build the circuit address for reservation by adding /p2p-circuit
                                    let circuit_addr = addr.with(Protocol::P2pCircuit);

                                    match swarm.listen_on(circuit_addr.clone()) {
                                        Ok(listener_id) => {
                                            tracing::info!("Swarm successfully listening on address {} with listener id {}.", circuit_addr, listener_id);
                                        }
                                        Err(e) => {
                                            tracing::error!("Swarm failed to listen on address {}: {}", circuit_addr, e);
                                            let _ = event_sender.unbounded_send(Event::Error {
                                                msg: format!("{}", e)
                                            });
                                        }
                                    }
                                }
                                Command::DialPeer { addr } => {
                                    tracing::info!("Dialing peer: {}", addr);
                                    if let Err(e) = swarm.dial(addr) {
                                        let _ = event_sender.unbounded_send(Event::Error {
                                            msg: format!("Failed to dial peer: {}", e)
                                        });
                                    }
                                }
                            }
                        }
                    }

                    event = swarm.select_next_some() => {
                        match event {
                            SwarmEvent::ConnectionEstablished { peer_id, connection_id, endpoint, .. } => {
                             
                                let remote_addr = endpoint.get_remote_address().to_string();

                                if remote_addr.contains("/webrtc") {
                                    tracing::info!(
                                        "Connection established with: {} via {} (remote: WebRTC, connection_id: {})", 
                                        peer_id, remote_addr, connection_id
                                    );
                                }

                                let _ = event_sender.unbounded_send(Event::ConnectionEstablished {
                                    peer_id: peer_id.to_string()
                                });
                            }
                            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                                tracing::info!("Connection closed with {}: {:?}", peer_id, cause);
                            }
                            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                                tracing::error!("Outgoing connection error to {:?}: {}", peer_id, error);
                                let _ = event_sender.unbounded_send(Event::Error {
                                    msg: format!("Connection error: {}", error)
                                });
                            }
                            SwarmEvent::NewListenAddr { address, .. } => {
                                tracing::info!("Listening on: {}", address);
                                if address.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
                                    tracing::info!("Relay reservation address: {}", address);
                                    let _ = event_sender.unbounded_send(Event::ReservationCreated);
                                }
                            }
                            SwarmEvent::Behaviour(event) => {
                                match event {
                                    WebRTCBehaviourEvent::Webrtc(webrtc_event) => {
                                        match webrtc_event {
                                            browser::SignalingEvent::NewWebRTCConnection(_connection) => {
                                                tracing::info!("Successfully established WebRTC connection");
                                            }
                                            browser::SignalingEvent::WebRTCConnectionError(_error) => {
                                                tracing::error!("Failed to establish WebRTC connection.")
                                            }
                                        }
                                    }
                                    WebRTCBehaviourEvent::Ping(ping_event) => {
                                        match ping_event {
                                            ping::Event { peer, result: Ok(rtt), .. } => {
                                                let rtt_ms = rtt.as_millis() as f64;
                                                tracing::info!("Ping successful: from {}", peer);
                                                let _ = event_sender.unbounded_send(Event::PingSuccess {
                                                    peer_id: peer.to_string(),
                                                    rtt_ms,
                                                });
                                            }
                                            ping::Event { peer, result: Err(e), .. } => {
                                                tracing::error!("Ping failed to {}: {}", peer, e);
                                                let _ = event_sender.unbounded_send(Event::Error {
                                                    msg: format!("Ping failed to {}: {}", peer, e)
                                                });
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(BrowserTransport {
            cmd_sender,
            event_receiver: std::sync::Arc::new(futures::lock::Mutex::new(event_receiver)),
            peer_id: local_peer_id.to_string(),
        })
    }

    #[wasm_bindgen]
    pub async fn listen_on_relay(&self, relay_addr: &str) -> Result<(), JsValue> {
        let addr = Multiaddr::from_str(relay_addr)
            .map_err(|e| JsValue::from_str(&format!("Invalid relay addr: {}", e)))?;

        self.cmd_sender
            .unbounded_send(Command::ListenOnRelay { addr })
            .map_err(|e| JsValue::from_str(&format!("Failed to send command: {}", e)))?;

        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn peer_id(&self) -> String {
        self.peer_id.clone()
    }

    #[wasm_bindgen]
    pub async fn dial(&self, peer_addr: &str) -> Result<(), JsValue> {
        let addr = Multiaddr::from_str(peer_addr)
            .map_err(|e| JsValue::from_str(&format!("Invalid peer addr: {}", e)))?;

        self.cmd_sender
            .unbounded_send(Command::DialPeer { addr })
            .map_err(|e| JsValue::from_str(&format!("Failed to send command: {}", e)))?;

        Ok(())
    }

    #[wasm_bindgen]
    pub async fn next_event(&self) -> Result<JsValue, JsValue> {
        let mut receiver = self.event_receiver.lock().await;

        if let Some(event) = receiver.next().await {
            let obj = Object::new();

            match event {
                Event::ReservationCreated => {
                    Reflect::set(&obj, &"type".into(), &"reservationCreated".into())?;
                }
                Event::ConnectionEstablished { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"connectionEstablished".into())?;
                    Reflect::set(&obj, &"peerId".into(), &peer_id.into())?;
                }
                Event::PingSuccess { peer_id, rtt_ms } => {
                    Reflect::set(&obj, &"type".into(), &"pingSuccess".into())?;
                    Reflect::set(&obj, &"peerId".into(), &peer_id.into())?;
                    Reflect::set(&obj, &"rttMs".into(), &rtt_ms.into())?;
                }
                Event::Error { msg } => {
                    Reflect::set(&obj, &"type".into(), &"error".into())?;
                    Reflect::set(&obj, &"message".into(), &msg.into())?;
                }
            }

            Ok(obj.into())
        } else {
            Err(JsValue::from_str("No events available"))
        }
    }
}

#[wasm_bindgen]
pub fn generate_peer_id() -> String {
    let keypair = Keypair::generate_ed25519();
    keypair.public().to_peer_id().to_string()
}
