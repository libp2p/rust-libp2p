use std::{str::FromStr, sync::Arc};

use futures::{channel::mpsc, task::AtomicWaker, StreamExt};
use js_sys::{Object, Reflect};
use libp2p::{
    identify, identity::Keypair, multiaddr::Protocol, noise, ping, swarm::SwarmEvent, yamux,
    Multiaddr, PeerId, Swarm, Transport,
};
use libp2p_core::{muxing::StreamMuxerBox, upgrade::Version};
use libp2p_swarm::NetworkBehaviour;
use libp2p_webrtc_websys::browser::{
    self, Behaviour, SignalingConfig, Transport as BrowserWebrtcTransport,
};
use tracing;
use tracing_wasm;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default_with_config(
        tracing_wasm::WASMLayerConfigBuilder::new()
            .set_max_level(tracing::Level::DEBUG)
            .set_console_config(tracing_wasm::ConsoleConfig::ReportWithConsoleColor)
            .build(),
    );
}

#[wasm_bindgen]
pub fn initialize() {
    tracing_wasm::set_as_global_default_with_config(
        tracing_wasm::WASMLayerConfigBuilder::new()
            .set_max_level(tracing::Level::DEBUG)
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
    ListenForWebRTC,
}

#[derive(Debug, Clone)]
enum Event {
    ReservationCreated { address: String },
    RelayConnectionEstablished { peer_id: String },
    WebRTCConnectionEstablished { peer_id: String },
    //  PingSuccess { peer_id: String, rtt_ms: f64 },
    Error { msg: String },
}

#[derive(NetworkBehaviour)]
pub struct WebRTCBehaviour {
    relay: libp2p_relay::client::Behaviour,
    identify: identify::Behaviour,
    webrtc: Behaviour,
    //  ping: ping::Behaviour,
}

#[wasm_bindgen]
impl BrowserTransport {
    #[wasm_bindgen]
    pub fn new() -> Result<BrowserTransport, JsError> {
        let keypair = Keypair::generate_ed25519();
        let local_peer_id = keypair.public().to_peer_id();

        let transport_waker = Arc::new(AtomicWaker::new());

        // Create a relay transport and behaviour for the relay connection
        let (relay_transport, relay_behaviour) = libp2p_relay::client::new(local_peer_id);

        let relay_transport_upgraded = relay_transport
            .upgrade(Version::V1)
            .authenticate(noise::Config::new(&keypair)?)
            .multiplex(yamux::Config::default())
            .boxed();

        // Create a websocket transport to facilitate connection to the relay server
        let ws_transport = libp2p_websocket_websys::Transport::default()
            .upgrade(Version::V1)
            .authenticate(noise::Config::new(&keypair)?)
            .multiplex(yamux::Config::default())
            .boxed();

        let webrtc_config = libp2p_webrtc_websys::browser::Config {
            keypair: keypair.clone(),
        };

        // Configure finite signaling with appropriate timeouts
        let signaling_config = SignalingConfig::new(
            3,                                     // max retries
            std::time::Duration::from_millis(0),   // signaling delay
            std::time::Duration::from_millis(100), // connection check delay
            300,                                   // max connection checks (30 seconds)
            std::time::Duration::from_secs(10),    // ICE gathering timeout
            keypair.public().to_peer_id(),         // The local peer's peer_id
        );

        let (webrtc_transport, webrtc_behaviour) =
            BrowserWebrtcTransport::new(webrtc_config, signaling_config, transport_waker.clone(),);
        let webrtc_transport_boxed = webrtc_transport.boxed();

        // A WebRTC behaviour facilitating coordination between the relay connection and webrtc signaling
        let behaviour = WebRTCBehaviour {
            relay: relay_behaviour,
            webrtc: webrtc_behaviour,
            identify: identify::Behaviour::new(identify::Config::new(
                "browser-to-browser-webrtc/1.0.0".into(),
                keypair.public(),
            )),
            // ping: ping::Behaviour::new(
            //     ping::Config::new()
            //         .with_interval(std::time::Duration::from_millis(500))
            //         .with_timeout(std::time::Duration::from_secs(3)),
            // ),
        };

        // The final transport consisting of a webrtc, relay and websocket transport
        let final_transport = webrtc_transport_boxed
            .or_transport(relay_transport_upgraded)
            .or_transport(ws_transport)
            .map(|either_output, _| match either_output {
                // WebRTC output (leftmost left)
                futures::future::Either::Left(futures::future::Either::Left((
                    peer_id,
                    connection,
                ))) => (peer_id, StreamMuxerBox::new(connection)),
                // Relay output (leftmost right)
                futures::future::Either::Left(futures::future::Either::Right(output)) => output,
                // WebSocket output (right)
                futures::future::Either::Right(output) => output,
            })
            .boxed();

        let mut swarm = Swarm::new(
            final_transport,
            behaviour,
            local_peer_id,
            libp2p::swarm::Config::with_executor(Box::new(|fut| {
                wasm_bindgen_futures::spawn_local(fut);
            }))
            .with_idle_connection_timeout(std::time::Duration::from_secs(10000)),
        );

        let (cmd_sender, mut cmd_receiver) = mpsc::unbounded();
        let (event_sender, event_receiver) = mpsc::unbounded();

        spawn_local(async move {
            let mut relay_address: Option<Multiaddr> = None;
            let mut webrtc_listening = false;

            loop {
                futures::select! {
                                    cmd = cmd_receiver.next() => {
                                        if let Some(cmd) = cmd {
                                            match cmd {
                                                Command::ListenOnRelay { addr } => {
                                                    relay_address = Some(addr.clone());
                                                    // Build the circuit address for reservation
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

                    // Check if this is a WebRTC circuit relay address
                    if addr.to_string().contains("/p2p-circuit") && addr.to_string().contains("/webrtc") {
                        // Remove /webrtc from the address to dial the relay circuit
                        let addr_str = addr.to_string();
                        let relay_circuit_addr_str = addr_str.replace("/webrtc", "");

                        tracing::info!("Transformed to relay circuit address: {}", relay_circuit_addr_str);

                        match relay_circuit_addr_str.parse::<Multiaddr>() {
                            Ok(relay_circuit_addr) => {
                                tracing::info!("Dialing relay circuit: {}", relay_circuit_addr);
                                if let Err(e) = swarm.dial(relay_circuit_addr) {
                                    let _ = event_sender.unbounded_send(Event::Error {
                                        msg: format!("Failed to dial relay circuit: {}", e)
                                    });
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse relay circuit address: {}", e);
                                let _ = event_sender.unbounded_send(Event::Error {
                                    msg: format!("Invalid relay circuit address: {}", e)
                                });
                            }
                        }
                    } else {
                        // Direct dial for non-relay addresses
                        if let Err(e) = swarm.dial(addr) {
                            let _ = event_sender.unbounded_send(Event::Error {
                                msg: format!("Failed to dial peer: {}", e)
                            });
                        }
                    }
                }
                                                Command::ListenForWebRTC => {
                                                    if !webrtc_listening {
                                                        // Create a virtual WebRTC listener address
                                                         let webrtc_listen_addr = "/webrtc".parse::<Multiaddr>().unwrap();

                                                        match swarm.listen_on(webrtc_listen_addr.clone()) {
                                                            Ok(listener_id) => {
                                                                 tracing::info!("WebRTC listener created with id: {}", listener_id);
                                                                webrtc_listening = true;

                                                            }
                                                            Err(e) => {
                                                                tracing::error!("Failed to create WebRTC listener: {}", e);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    event = swarm.select_next_some() => {
                                        match event {
                                            SwarmEvent::ConnectionEstablished { peer_id, connection_id, endpoint, .. } => {
                                                let remote_addr = endpoint.get_remote_address().to_string();

                                                if remote_addr.contains("/webrtc") && !remote_addr.contains("/p2p-circuit") {
                                                    tracing::info!(
                                                        "Direct WebRTC connection established with: {} via {} (connection_id: {})",
                                                        peer_id, remote_addr, connection_id
                                                    );

                                                    let _ = event_sender.unbounded_send(Event::WebRTCConnectionEstablished {
                                                        peer_id: peer_id.to_string()
                                                    });
                                                } else if remote_addr.contains("/p2p-circuit") {
                                                    tracing::info!(
                                                        "Relay connection established with: {} via {} (connection_id: {})",
                                                        peer_id, remote_addr, connection_id
                                                    );

                                                    let _ = event_sender.unbounded_send(Event::RelayConnectionEstablished {
                                                        peer_id: peer_id.to_string()
                                                    });
                                                }
                                            }
                                            SwarmEvent::ConnectionClosed { peer_id, cause, endpoint, connection_id, .. } => {
                                                tracing::info!("Connection on endpoint {:?} closed with cause: {:?}", endpoint, cause);

                                                // let addr = endpoint.get_remote_address().to_string();
                                                // if addr.contains("/p2p-circuit") && !addr.contains("/webrtc") {
                                                //     tracing::info!("Relay connection closed with {}: {:?}", peer_id, cause);
                                                // } else if addr.contains("/webrtc") {
                                                //     tracing::info!("WebRTC connection closed with {}: {:?}", peer_id, cause);
                                                // }

                                                  // Query the swarm for remaining connections to this peer
                    let remaining = swarm.network_info().num_peers();

                    // Better: Check if we still have ANY connection to this specific peer
                    let still_connected = swarm.is_connected(&peer_id);

                    tracing::info!(
                        "Connection {} closed to {} (still_connected: {}, total_peers: {})",
                        connection_id,
                        peer_id,
                        still_connected,
                        remaining
                    );

                    if !still_connected {
                        tracing::warn!("Peer {} fully disconnected - all connections closed", peer_id);
                    }
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
                                                    // Generate the complete WebRTC address
                                                    if let Some(relay_addr) = &relay_address {
                                                        let webrtc_addr = format!(
                                                            "{}/p2p-circuit/webrtc/p2p/{}",
                                                            relay_addr,
                                                            local_peer_id
                                                        );
                                                        tracing::info!("WebRTC address: {}", webrtc_addr);
                                                        let _ = event_sender.unbounded_send(Event::ReservationCreated {
                                                            address: webrtc_addr,
                                                        });
                                                    }
                                                }
                                            }
                                            SwarmEvent::Behaviour(event) => {
                                                match event {
                                                    WebRTCBehaviourEvent::Webrtc(webrtc_event) => {
                                                        match webrtc_event {
                                                            browser::SignalingEvent::NewWebRTCConnection { peer_id } => {
                                                                tracing::info!("WebRTC connection established with peer: {}", peer_id);
                                                                // Connection is now managed by the swarm
                                                            }
                                                            browser::SignalingEvent::WebRTCConnectionError { peer_id, error } => {
                                                                tracing::error!("Failed to establish WebRTC connection with {}: {:?}", peer_id, error);
                                                                let _ = event_sender.unbounded_send(Event::Error {
                                                                    msg: format!("WebRTC error with {}: {}", peer_id, error)
                                                                });
                                                            }
                                                        }
                                                    }
                                                    // WebRTCBehaviourEvent::Ping(ping_event) => {
                                                    //     match ping_event {
                                                    //         ping::Event { peer, result: Ok(rtt), .. } => {
                                                    //             let rtt_ms = rtt.as_millis() as f64;
                                                    //             let _ = event_sender.unbounded_send(Event::PingSuccess {
                                                    //                 peer_id: peer.to_string(),
                                                    //                 rtt_ms,
                                                    //             });
                                                    //         }
                                                    //         ping::Event { peer, result: Err(e), .. } => {
                                                    //             let _ = event_sender.unbounded_send(Event::Error {
                                                    //                 msg: format!("Ping failed to {}: {}", peer, e)
                                                    //             });
                                                    //         }
                                                    //     }
                                                    // }
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

        // Also setup WebRTC listener
        self.cmd_sender
            .unbounded_send(Command::ListenForWebRTC)
            .map_err(|e| JsValue::from_str(&format!("Failed to setup WebRTC listener: {}", e)))?;

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
                Event::ReservationCreated { address } => {
                    Reflect::set(&obj, &"type".into(), &"reservationCreated".into())?;
                    Reflect::set(&obj, &"address".into(), &address.into())?;
                }
                Event::RelayConnectionEstablished { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"relayConnectionEstablished".into())?;
                    Reflect::set(&obj, &"peerId".into(), &peer_id.into())?;
                }
                Event::WebRTCConnectionEstablished { peer_id } => {
                    Reflect::set(&obj, &"type".into(), &"webrtcConnectionEstablished".into())?;
                    Reflect::set(&obj, &"peerId".into(), &peer_id.into())?;
                }
                // Event::PingSuccess { peer_id, rtt_ms } => {
                //     Reflect::set(&obj, &"type".into(), &"pingSuccess".into())?;
                //     Reflect::set(&obj, &"peerId".into(), &peer_id.into())?;
                //     Reflect::set(&obj, &"rttMs".into(), &rtt_ms.into())?;
                // }
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
