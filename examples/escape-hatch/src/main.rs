
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




async fn test() {

    let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
    let mut transport = MemoryTransport::new().boxed();
    transport.listen_on(ListenerId::next(), mem_addr).unwrap();

    let listener_addr = transport
        .select_next_some()
        .now_or_never()
        .and_then(|ev| ev.into_new_address())
        .expect("MemoryTransport not listening on an address!");

    async_std::task::spawn(async move {
        let transport_event = transport.next().await.unwrap();
        let (listener_upgrade, _) = transport_event.into_incoming().unwrap();
        let conn = listener_upgrade.await.unwrap();
        // recv_ping(conn).await.unwrap();
    });

    async_std::task::block_on(async move {
        let c = MemoryTransport::new()
            .dial(listener_addr)
            .unwrap()
            .await
            .unwrap();
        // let (_, rtt) = send_ping(c).await.unwrap();
    });
}
