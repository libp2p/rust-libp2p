//! # Composing `NetworkBehaviour`s
//!
//! Manually composing behaviours is more straightforward,
//! you combine the behaviours in a struct and delegate calls,
//! then combine their respective `ToSwarm` events in an enum.
//!
//! ```
//! use libp2p::{identify, mdns, ping};
//!
//! struct Behaviour {
//!     identify: identify::Behaviour,
//!     ping: ping::Behaviour,
//! }
//! ```

use derive_more::From;
use libp2p::{identify, mdns, ping, swarm::NetworkBehaviour};

use crate::connection_handler::Handler;

struct Behaviour {
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

#[derive(From)]
enum ToSwarm {
    #[from]
    Identify(identify::Event),
    #[from]
    Ping(ping::Event),
    #[from]
    Mdns(mdns::tokio::Behaviour),
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = crate::connection_handler::Handler;

    type ToSwarm = ToSwarm;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: libp2p::PeerId,
        local_addr: &libp2p::Multiaddr,
        remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        let ping_handler = self.ping.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )?;
        let identify_handler = self.identify.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )?;
        let mdns_handler = self.mdns.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )?;
        Ok(Handler {
            ping: ping_handler,
            identify: identify_handler,
            mdns: mdns_handler,
        })
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: libp2p::PeerId,
        addr: &libp2p::Multiaddr,
        role_override: libp2p::core::Endpoint,
        port_use: libp2p::core::transport::PortUse,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        let ping_handler = self.ping.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )?;
        let identify_handler = self.identify.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )?;
        let mdns_handler = self.mdns.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )?;
        Ok(Handler {
            ping: ping_handler,
            identify: identify_handler,
            mdns: mdns_handler,
        })
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        self.identify.on_swarm_event(event);
        self.mdns.on_swarm_event(event);
        self.ping.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: libp2p::PeerId,
        connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        match event {}
        NetworkBehaviour::on_connection_handler_event(
            &mut self.identify,
            peer_id,
            connection_id,
            event,
        );
        NetworkBehaviour::on_connection_handler_event(
            &mut self.mdns,
            peer_id,
            connection_id,
            event,
        );
        NetworkBehaviour::on_connection_handler_event(
            &mut self.ping,
            peer_id,
            connection_id,
            event,
        );
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        todo!()
    }
}
