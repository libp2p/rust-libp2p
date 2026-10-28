use std::{
    collections::{HashSet, VecDeque},
    convert::Infallible,
    task::{Context, Poll},
    time::Duration,
};

use libp2p_core::{Endpoint, Multiaddr, transport::PortUse, upgrade::DeniedUpgrade};
use libp2p_identity::PeerId;
use libp2p_kad::{Behaviour, Event, Mode, PROTOCOL_NAME, store::MemoryStore};
use libp2p_swarm::{
    ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent, ConnectionId, FromSwarm,
    NetworkBehaviour, StreamProtocol, SubstreamProtocol, Swarm, SwarmEvent, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
    handler::{ConnectionEvent, ProtocolSupport},
};
use libp2p_swarm_test::SwarmExt;

/// A behaviour whose handler reports two changes of the remote's protocols
/// right after the connection is established: first that the remote supports
/// kademlia, then that it supports some unrelated protocol.
///
/// This mimics e.g. `libp2p-identify` emitting an `Added` and a `Removed`
/// report back-to-back.
struct Reporter;

struct ReporterHandler {
    reports: VecDeque<ProtocolSupport>,
}

impl NetworkBehaviour for Reporter {
    type ConnectionHandler = ReporterHandler;
    type ToSwarm = Infallible;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(ReporterHandler::new())
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(ReporterHandler::new())
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        e: THandlerOutEvent<Self>,
    ) {
        match e {}
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }

    fn on_swarm_event(&mut self, _: FromSwarm) {}
}

impl ReporterHandler {
    fn new() -> Self {
        Self {
            reports: VecDeque::from([
                ProtocolSupport::Added(HashSet::from([PROTOCOL_NAME])),
                ProtocolSupport::Added(HashSet::from([StreamProtocol::new("/unrelated/1.0.0")])),
            ]),
        }
    }
}

impl ConnectionHandler for ReporterHandler {
    type FromBehaviour = Infallible;
    type ToBehaviour = Infallible;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn on_behaviour_event(&mut self, e: Self::FromBehaviour) {
        match e {}
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        match self.reports.pop_front() {
            Some(report) => Poll::Ready(ConnectionHandlerEvent::ReportRemoteProtocols(report)),
            None => Poll::Pending,
        }
    }

    fn on_connection_event(
        &mut self,
        _: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct MyBehaviour {
    // Polled before `kad`.
    reporter: Reporter,
    kad: Behaviour<MemoryStore>,
}

impl MyBehaviour {
    fn new(k: libp2p_identity::Keypair) -> Self {
        let local_peer_id = k.public().to_peer_id();
        let mut kad = Behaviour::new(local_peer_id, MemoryStore::new(local_peer_id));
        kad.set_mode(Some(Mode::Client));
        Self {
            reporter: Reporter,
            kad,
        }
    }
}

#[tokio::test]
async fn remote_is_added_to_routing_table_despite_consecutive_protocol_reports() {
    let mut client = Swarm::new_ephemeral_tokio(MyBehaviour::new);
    let mut server = Swarm::new_ephemeral_tokio(MyBehaviour::new);
    let server_peer_id = *server.local_peer_id();

    server.listen().with_memory_addr_external().await;
    client.connect(&mut server).await;
    tokio::spawn(server.loop_on_next());

    let routing_updated = tokio::time::timeout(
        Duration::from_secs(3),
        client.wait(|e| match e {
            SwarmEvent::Behaviour(MyBehaviourEvent::Kad(Event::RoutingUpdated {
                peer, ..
            })) => Some(peer),
            _ => None,
        }),
    )
    .await
    .expect("server to be added to the client's routing table");

    assert_eq!(routing_updated, server_peer_id);
}
