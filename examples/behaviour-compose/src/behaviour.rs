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
//!     mdns: mdns::tokio::Behaviour, // using `tokio` runtime
//! }
//! ```

use std::task::Poll;

use libp2p::{identify, mdns, ping, swarm::NetworkBehaviour};

use crate::connection_handler::Handler;

pub struct Behaviour {
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

pub enum ToSwarm {
    Identify(identify::Event),
    Ping(ping::Event),
    Mdns(mdns::Event),
}

macro_rules! poll_behaviour {
    (($self:ident,$cx:ident)=>{$($name:ident:$behaviour:ident,)+}) => {
        $(
            // I know it looks kind of dumb but the type system requires this
            // to get rid of the generics.
            match ::libp2p::swarm::NetworkBehaviour::poll(
                &mut $self.$name,
                $cx,
            ){
                std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::GenerateEvent(
                        event,
                    ),
                ) => {
                    return std::task::Poll::Ready(
                        ::libp2p::swarm::derive_prelude::ToSwarm::GenerateEvent(
                            ToSwarm::$behaviour(event),
                        ),
                    );
                }
                std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::Dial { opts },
                ) => {
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::Dial {
                        opts,
                    });
                }
                std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::ListenOn { opts },
                ) => {
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::ListenOn {
                        opts,
                    });
                }
                std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::RemoveListener{id})=>{
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::RemoveListener {
                        id
                    });
                }
                std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::NotifyHandler {
                        peer_id,
                        handler,
                        event,
                    },
                ) => {
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::NotifyHandler {
                        peer_id,
                        handler,
                        event: crate::connection_handler::FromBehaviour::$behaviour(event)
                    });
                }
                std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrCandidate(addr))=>{
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrCandidate(addr));
                }
                std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrConfirmed(addr))=>{
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrConfirmed(addr));
                }
                std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrExpired(addr))=>{
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrExpired(addr));
                }
                std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    },
                ) => {
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    });
                }
                std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrOfPeer{peer_id,address})=>{
                    return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrOfPeer{peer_id,address});
                }
                std::task::Poll::Ready(uncovered)=>{
                    unimplemented!("New branch {:?} not covered", uncovered)
                }
                std::task::Poll::Pending => {}
            }
        )*
    };
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
        // Yes, the entire connection will be denied when any of the behaviours rejects.
        // I'm not sure what will happen if the connection is partially rejected, e.g. making
        // `Handler` containing `Option`s.
        Ok(Handler {
            ping: ping_handler,
            identify: identify_handler,
            mdns: mdns_handler,
        })
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        // `FromSwarm` needs to be broadcast to all `NetworkBehaviour`s.
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
        // Events collected from the `Handler` need to be delegated to the corresponding `ConnectionHandler`.
        use super::connection_handler::ToBehaviour::*;
        match event {
            Ping(ev) => self
                .ping
                .on_connection_handler_event(peer_id, connection_id, ev),
            Mdns(ev) => self
                .mdns
                .on_connection_handler_event(peer_id, connection_id, ev),
            Identify(ev) => self
                .identify
                .on_connection_handler_event(peer_id, connection_id, ev),
        }
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        // Yes, composed behaviour has a hidden polling order, but is not a hard one.
        // You can change the order they're polled by rearrange the statements,
        // though the order can be changed by the compiler.
        // Here we need to properly delegate the events.

        match ::libp2p::swarm::NetworkBehaviour::poll(&mut self.ping, cx) {
            // `ToSwarm::GenerateEvent` needs to be aggregated.
            std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::GenerateEvent(
                event,
            )) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::GenerateEvent(ToSwarm::Ping(event)),
                );
            }
            // `ToSwarm::NotifyHandler` needs to be delegated.
            std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::NotifyHandler {
                peer_id,
                handler,
                event,
            }) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::NotifyHandler {
                        peer_id,
                        handler,
                        event: crate::connection_handler::FromBehaviour::Ping(event),
                    },
                );
            }
            std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::Dial { opts }) => {
                return std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::Dial {
                    opts,
                });
            }
            std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::ListenOn { opts }) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::ListenOn { opts },
                );
            }
            std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::RemoveListener {
                id,
            }) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::RemoveListener { id },
                );
            }
            std::task::Poll::Ready(
                ::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrCandidate(addr),
            ) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrCandidate(addr),
                );
            }
            std::task::Poll::Ready(
                ::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrConfirmed(addr),
            ) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrConfirmed(addr),
                );
            }
            std::task::Poll::Ready(
                ::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrExpired(addr),
            ) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::ExternalAddrExpired(addr),
                );
            }
            std::task::Poll::Ready(::libp2p::swarm::derive_prelude::ToSwarm::CloseConnection {
                peer_id,
                connection,
            }) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::CloseConnection {
                        peer_id,
                        connection,
                    },
                );
            }
            std::task::Poll::Ready(
                ::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrOfPeer {
                    peer_id,
                    address,
                },
            ) => {
                return std::task::Poll::Ready(
                    ::libp2p::swarm::derive_prelude::ToSwarm::NewExternalAddrOfPeer {
                        peer_id,
                        address,
                    },
                );
            }
            std::task::Poll::Ready(uncovered) => {
                unimplemented!("New branch {:?} not covered", uncovered)
            }
            std::task::Poll::Pending => {}
        }
        poll_behaviour!((self, cx)=>{identify:Identify,mdns:Mdns,});
        Poll::Pending
    }
}
