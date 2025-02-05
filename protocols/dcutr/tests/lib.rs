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

use std::time::Duration;

use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{upgrade::Version, MemoryTransport, Transport},
};
use libp2p_dcutr as dcutr;
use libp2p_identify as identify;
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use libp2p_plaintext as plaintext;
use libp2p_relay as relay;
use libp2p_swarm::{Config, NetworkBehaviour, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt as _;
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn connect() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut relay = build_relay();
    let mut dst = build_client();
    let mut src = build_client();

    // Have all swarms listen on a local TCP address.
    let (_, relay_tcp_addr) = relay.listen().with_tcp_addr_external().await;
    let (_, dst_tcp_addr) = dst.listen().await;
    src.listen().await;

    assert!(src.external_addresses().next().is_none());
    assert!(dst.external_addresses().next().is_none());

    let relay_peer_id = *relay.local_peer_id();
    let dst_peer_id = *dst.local_peer_id();

    tokio::spawn(relay.loop_on_next());

    let dst_relayed_addr = relay_tcp_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(dst_peer_id));
    dst.listen_on(dst_relayed_addr.clone()).unwrap();

    wait_for_reservation(
        &mut dst,
        dst_relayed_addr.clone(),
        relay_peer_id,
        false, // No renewal.
    )
    .await;
    tokio::spawn(dst.loop_on_next());

    src.dial_and_wait(dst_relayed_addr.clone()).await;

    let dst_addr = dst_tcp_addr.with(Protocol::P2p(dst_peer_id));

    let established_conn_id = src
        .wait(move |e| match e {
            SwarmEvent::ConnectionEstablished {
                endpoint,
                connection_id,
                ..
            } => (*endpoint.get_remote_address() == dst_addr).then_some(connection_id),
            _ => None,
        })
        .await;

    let reported_conn_id = src
        .wait(move |e| match e {
            SwarmEvent::Behaviour(ClientEvent::Dcutr(dcutr::Event {
                result: Ok(connection_id),
                ..
            })) => Some(connection_id),
            _ => None,
        })
        .await;

    assert_eq!(established_conn_id, reported_conn_id);
}

fn build_relay() -> Swarm<Relay> {
    Swarm::new_ephemeral(|identity| {
        let local_peer_id = identity.public().to_peer_id();

        Relay {
            relay: relay::Behaviour::new(
                local_peer_id,
                relay::Config {
                    reservation_duration: Duration::from_secs(2),
                    ..Default::default()
                },
            ),
            identify: identify::Behaviour::new(identify::Config::new(
                "/relay".to_owned(),
                identity.public(),
            )),
        }
    })
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Relay {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
}

fn build_client() -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();

    let (relay_transport, behaviour) = relay::client::new(local_peer_id);

    let transport = relay_transport
        .or_transport(MemoryTransport::default())
        .or_transport(libp2p_tcp::async_io::Transport::default())
        .upgrade(Version::V1)
        .authenticate(plaintext::Config::new(&local_key))
        .multiplex(libp2p_yamux::Config::default())
        .boxed();

    Swarm::new(
        transport,
        Client {
            relay: behaviour,
            dcutr: dcutr::Behaviour::new(local_peer_id),
            identify: identify::Behaviour::new(identify::Config::new(
                "/client".to_owned(),
                local_key.public(),
            )),
        },
        local_peer_id,
        Config::with_async_std_executor(),
    )
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Client {
    relay: relay::client::Behaviour,
    dcutr: dcutr::Behaviour,
    identify: identify::Behaviour,
}

async fn wait_for_reservation(
    client: &mut Swarm<Client>,
    client_addr: Multiaddr,
    relay_peer_id: PeerId,
    is_renewal: bool,
) {
    let mut new_listen_addr_for_relayed_addr = false;
    let mut reservation_req_accepted = false;
    let mut addr_observed = false;

    loop {
        if new_listen_addr_for_relayed_addr && reservation_req_accepted && addr_observed {
            break;
        }

        match client.next_swarm_event().await {
            SwarmEvent::NewListenAddr { address, .. } if address == client_addr => {
                new_listen_addr_for_relayed_addr = true;
            }
            SwarmEvent::Behaviour(ClientEvent::Relay(
                relay::client::Event::ReservationReqAccepted {
                    relay_peer_id: peer_id,
                    renewal,
                    ..
                },
            )) if relay_peer_id == peer_id && renewal == is_renewal => {
                reservation_req_accepted = true;
            }
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } if peer_id == relay_peer_id => {}
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == relay_peer_id => {}
            SwarmEvent::Behaviour(ClientEvent::Identify(identify::Event::Received { .. })) => {
                addr_observed = true;
            }
            SwarmEvent::Behaviour(ClientEvent::Identify(_)) => {}
            SwarmEvent::NewExternalAddrCandidate { .. } => {}
            SwarmEvent::ExternalAddrConfirmed { address } if !is_renewal => {
                assert_eq!(address, client_addr);
            }
            SwarmEvent::NewExternalAddrOfPeer { .. } => {}
            e => panic!("{e:?}"),
        }
    }
}
