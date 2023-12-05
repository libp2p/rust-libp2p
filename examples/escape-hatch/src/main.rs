
use futures::prelude::*;
use futures::StreamExt;
use libp2p::StreamProtocol;
use libp2p_core::{
    multiaddr::multiaddr,
    transport::{memory::MemoryTransport, ListenerId, Transport},
};
use rand::{thread_rng, Rng};
use escape_hatch::Behaviour;

#[tokio::main]
async fn main() {
    let stream_protocol = StreamProtocol::new("/escape-hatch/");
    let (behaviour, control, incoming_stream) = Behaviour::new(stream_protocol);
}
