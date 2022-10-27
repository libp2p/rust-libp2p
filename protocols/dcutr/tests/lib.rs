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

use futures::stream::StreamExt;
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::transport::upgrade::Version;
use libp2p::core::transport::{MemoryTransport, OrTransport, Transport};
use libp2p::core::{identity, PeerId};
use libp2p::dcutr;
use libp2p::plaintext::PlainText2Config;
use libp2p::relay::v2::client;
use libp2p::relay::v2::relay;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::NetworkBehaviour;
use libp2p_swarm_test::SwarmExt;
use std::time::Duration;

#[async_std::test]
async fn connect() {
    let _ = env_logger::try_init();

    let mut relay = build_relay();
    let mut dst = build_client();
    let mut src = build_client();

    // Have all swarms listen on a local memory address.
    let relay_addr = relay.listen().await;
    let dst_addr = dst.listen().await;
    src.listen().await;

    let relay_peer_id = *relay.local_peer_id();
    let dst_peer_id = *dst.local_peer_id();

    async_std::task::spawn(relay.loop_on_next());

    let dst_relayed_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(dst_peer_id.into()));
    dst.listen_on(dst_relayed_addr.clone()).unwrap();

    wait_for_reservation(
        &mut dst,
        dst_relayed_addr.clone(),
        relay_peer_id,
        false, // No renewal.
    )
    .await;
    async_std::task::spawn(dst.loop_on_next());

    src.dial_and_wait(dst_relayed_addr.clone()).await;

    loop {
        match src
            .next_or_timeout()
            .await
            .try_into_behaviour_event()
            .unwrap()
        {
            ClientEvent::Dcutr(
                dcutr::behaviour::Event::RemoteInitiatedDirectConnectionUpgrade {
                    remote_peer_id,
                    remote_relayed_addr,
                },
            ) => {
                if remote_peer_id == dst_peer_id && remote_relayed_addr == dst_relayed_addr {
                    break;
                }
            }
            other => panic!("Unexpected event: {:?}.", other),
        }
    }

    let dst_addr = dst_addr.with(Protocol::P2p(dst_peer_id.into()));

    src.wait(move |e| match e {
        SwarmEvent::ConnectionEstablished { endpoint, .. } => {
            (*endpoint.get_remote_address() == dst_addr).then(|| ())
        }
        _ => None,
    })
    .await;
}

fn build_relay() -> Swarm<relay::Relay> {
    Swarm::new_ephemeral(|identity| {
        let local_peer_id = identity.public().to_peer_id();

        relay::Relay::new(
            local_peer_id,
            relay::Config {
                reservation_duration: Duration::from_secs(2),
                ..Default::default()
            },
        )
    })
}

fn build_client() -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let local_peer_id = local_public_key.to_peer_id();

    let (relay_transport, behaviour) = client::Client::new_transport_and_behaviour(local_peer_id);

    let transport = OrTransport::new(relay_transport, MemoryTransport::default())
        .boxed()
        .upgrade(Version::V1)
        .authenticate(PlainText2Config { local_public_key })
        .multiplex(libp2p::yamux::YamuxConfig::default())
        .boxed();

    Swarm::new(
        transport,
        Client {
            relay: behaviour,
            dcutr: dcutr::behaviour::Behaviour::new(),
        },
        local_peer_id,
    )
}

#[derive(NetworkBehaviour)]
struct Client {
    relay: client::Client,
    dcutr: dcutr::behaviour::Behaviour,
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
