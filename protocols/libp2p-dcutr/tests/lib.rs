// Copyright 2021 Protocol Labs.
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

use futures::executor::LocalPool;
use futures::future::FutureExt;
use futures::io::{AsyncRead, AsyncWrite};
use futures::stream::StreamExt;
use futures::task::Spawn;
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::upgrade::Version;
use libp2p::core::transport::{Boxed, MemoryTransport, OrTransport, Transport};
use libp2p::core::PublicKey;
use libp2p::core::{identity, PeerId};
use libp2p::dcutr;
use libp2p::plaintext::PlainText2Config;
use libp2p::relay::v2::client;
use libp2p::relay::v2::relay;
use libp2p::swarm::{AddressScore, NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::NetworkBehaviour;
use std::time::Duration;

#[test]
fn connect() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone(), AddressScore::Infinite);
    spawn_swarm_on_pool(&pool, relay);

    let mut dst = build_client();
    let dst_peer_id = *dst.local_peer_id();
    let dst_relayed_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(dst_peer_id.into()));
    let dst_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));

    dst.listen_on(dst_relayed_addr.clone()).unwrap();
    dst.listen_on(dst_addr.clone()).unwrap();
    dst.add_external_address(dst_addr.clone(), AddressScore::Infinite);

    pool.run_until(wait_for_reservation(
        &mut dst,
        dst_relayed_addr.clone(),
        relay_peer_id,
        false, // No renewal.
    ));
    spawn_swarm_on_pool(&pool, dst);

    let mut src = build_client();
    let src_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    src.listen_on(src_addr.clone()).unwrap();
    pool.run_until(wait_for_new_listen_addr(&mut src, &src_addr));
    src.add_external_address(src_addr.clone(), AddressScore::Infinite);

    src.dial(dst_relayed_addr.clone()).unwrap();

    pool.run_until(wait_for_connection_established(&mut src, &dst_relayed_addr));
    match pool.run_until(wait_for_dcutr_event(&mut src)) {
        dcutr::behaviour::Event::RemoteInitiatedDirectConnectionUpgrade {
            remote_peer_id,
            remote_relayed_addr,
        } if remote_peer_id == dst_peer_id && remote_relayed_addr == dst_relayed_addr => {}
        e => panic!("Unexpected event: {:?}.", e),
    }
    pool.run_until(wait_for_connection_established(
        &mut src,
        &dst_addr.with(Protocol::P2p(dst_peer_id.into())),
    ));
}

fn build_relay() -> Swarm<relay::Relay> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let local_peer_id = local_public_key.to_peer_id();

    let transport = build_transport(MemoryTransport::default().boxed(), local_public_key);

    Swarm::new(
        transport,
        relay::Relay::new(
            local_peer_id,
            relay::Config {
                reservation_duration: Duration::from_secs(2),
                ..Default::default()
            },
        ),
        local_peer_id,
    )
}

fn build_client() -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let local_peer_id = local_public_key.to_peer_id();

    let (relay_transport, behaviour) = client::Client::new_transport_and_behaviour(local_peer_id);
    let transport = build_transport(
        OrTransport::new(relay_transport, MemoryTransport::default()).boxed(),
        local_public_key,
    );

    Swarm::new(
        transport,
        Client {
            relay: behaviour,
            dcutr: dcutr::behaviour::Behaviour::new(),
        },
        local_peer_id,
    )
}

fn build_transport<StreamSink>(
    transport: Boxed<StreamSink>,
    local_public_key: PublicKey,
) -> Boxed<(PeerId, StreamMuxerBox)>
where
    StreamSink: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    transport
        .upgrade(Version::V1)
        .authenticate(PlainText2Config { local_public_key })
        .multiplex(libp2p::yamux::YamuxConfig::default())
        .boxed()
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "ClientEvent", event_process = false)]
struct Client {
    relay: client::Client,
    dcutr: dcutr::behaviour::Behaviour,
}

#[derive(Debug)]
enum ClientEvent {
    Relay(client::Event),
    Dcutr(dcutr::behaviour::Event),
}

impl From<client::Event> for ClientEvent {
    fn from(event: client::Event) -> Self {
        ClientEvent::Relay(event)
    }
}

impl From<dcutr::behaviour::Event> for ClientEvent {
    fn from(event: dcutr::behaviour::Event) -> Self {
        ClientEvent::Dcutr(event)
    }
}

fn spawn_swarm_on_pool<B: NetworkBehaviour + Send>(pool: &LocalPool, swarm: Swarm<B>) {
    pool.spawner()
        .spawn_obj(swarm.collect::<Vec<_>>().map(|_| ()).boxed().into())
        .unwrap();
}

async fn wait_for_reservation(
    client: &mut Swarm<Client>,
    client_addr: Multiaddr,
    relay_peer_id: PeerId,
    is_renewal: bool,
) {
    let mut new_listen_addr_for_relayed_addr = false;
    let mut reservation_req_accepted = false;
    loop {
        match client.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } if address != client_addr => {}
            SwarmEvent::NewListenAddr { address, .. } if address == client_addr => {
                new_listen_addr_for_relayed_addr = true;
                if reservation_req_accepted {
                    break;
                }
            }
            SwarmEvent::Behaviour(ClientEvent::Relay(client::Event::ReservationReqAccepted {
                relay_peer_id: peer_id,
                renewal,
                ..
            })) if relay_peer_id == peer_id && renewal == is_renewal => {
                reservation_req_accepted = true;
                if new_listen_addr_for_relayed_addr {
                    break;
                }
            }
            SwarmEvent::Dialing(peer_id) if peer_id == relay_peer_id => {}
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == relay_peer_id => {}
            e => panic!("{:?}", e),
        }
    }
}

async fn wait_for_connection_established(client: &mut Swarm<Client>, addr: &Multiaddr) {
    loop {
        match client.select_next_some().await {
            SwarmEvent::IncomingConnection { .. } => {}
            SwarmEvent::ConnectionEstablished { endpoint, .. }
                if endpoint.get_remote_address() == addr =>
            {
                break
            }
            SwarmEvent::Dialing(_) => {}
            SwarmEvent::Behaviour(ClientEvent::Relay(
                client::Event::OutboundCircuitEstablished { .. },
            )) => {}
            SwarmEvent::ConnectionEstablished { .. } => {}
            e => panic!("{:?}", e),
        }
    }
}

async fn wait_for_new_listen_addr(client: &mut Swarm<Client>, new_addr: &Multiaddr) {
    match client.select_next_some().await {
        SwarmEvent::NewListenAddr { address, .. } if address == *new_addr => {}
        e => panic!("{:?}", e),
    }
}

async fn wait_for_dcutr_event(client: &mut Swarm<Client>) -> dcutr::behaviour::Event {
    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(ClientEvent::Dcutr(e)) => return e,
            e => panic!("{:?}", e),
        }
    }
}
