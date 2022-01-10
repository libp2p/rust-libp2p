mod util;

use std::task::Poll;

use libp2p_core::{
    connection::{self, PendingOutboundConnectionError},
    network::{NetworkConfig, NetworkEvent},
    transport::dummy::DummyTransport,
    Multiaddr, Network, PeerId,
};

use futures::{executor::block_on, future::poll_fn};
use multihash::Multihash;

#[test]
fn aborting_pending_connection_surfaces_error() {
    let mut network = Network::new(
        DummyTransport::default(),
        PeerId::random(),
        NetworkConfig::default(),
    );

    let target_peer = PeerId::random();
    let mut target_multiaddr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap();
    target_multiaddr.push(multiaddr::Protocol::P2p(target_peer.into()));

    let handler = util::TestHandler();
    network
        .dial(&target_multiaddr, handler)
        .expect("dial failed");

    let dialing_peer = network
        .peer(target_peer)
        .into_dialing()
        .expect("peer should be dialing");

    dialing_peer.disconnect();
    block_on(poll_fn(|cx| match network.poll(cx) {
        Poll::Pending => return Poll::Pending,
        Poll::Ready(NetworkEvent::DialError {
            error: PendingOutboundConnectionError::Aborted,
            ..
        }) => {
            return Poll::Ready(());
        }
        Poll::Ready(_) => {
            panic!("We should see an aborted error, nothing else.")
        }
    }));
}
