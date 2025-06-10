use libp2p_swarm::StreamProtocol;

pub const SIGNALING_PROTOCOL_ID: &'static str = "/webrtc-signaling/0.0.1";
pub const SIGNALING_STREAM_PROTOCOL: StreamProtocol = StreamProtocol::new(SIGNALING_PROTOCOL_ID);
