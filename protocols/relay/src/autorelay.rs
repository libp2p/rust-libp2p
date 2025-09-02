use crate::autorelay::handler::Out;
use crate::multiaddr_ext::MultiaddrExt;
use either::Either;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::transport::{ListenerId, PortUse};
use libp2p_core::Endpoint;
use libp2p_identity::PeerId;
use libp2p_swarm::derive_prelude::{
    AddressChange, ConnectionClosed, ConnectionDenied, ConnectionEstablished, ConnectionId,
    ExpiredListenAddr, FromSwarm, ListenerClosed, ListenerError, Multiaddr, NetworkBehaviour,
    THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p_swarm::{dummy, ExternalAddresses, ListenOpts, NewListenAddr};
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroU8;
use std::task::{Context, Poll, Waker};

mod handler;

#[derive(Default, Debug)]
pub struct Behaviour {
    config: Config,
    external_addresses: ExternalAddresses,
    events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,

    connections: HashMap<(PeerId, ConnectionId), Connection>,

    reservations: HashMap<ListenerId, (PeerId, ConnectionId)>,
    pending_target: HashSet<(PeerId, ConnectionId)>,

    waker: Option<Waker>,
}

#[derive(Debug)]
struct Connection {
    address: Multiaddr,
    relay_status: RelayStatus,
}

impl Connection {
    /// Mark relayed connection as not supported
    pub(crate) fn disqualify_connection_if_relayed(&mut self) -> bool {
        match self.address.is_relayed() {
            true => {
                self.relay_status = RelayStatus::NotSupported;
                true
            }
            false => {
                self.relay_status = RelayStatus::Pending;
                false
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelayStatus {
    Supported { status: ReservationStatus },
    NotSupported,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReservationStatus {
    Idle,
    Pending { id: ListenerId },
    Active { id: ListenerId },
}

#[derive(Debug)]
pub struct Config {
    max_reservations: NonZeroU8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_reservations: NonZeroU8::new(2).unwrap(),
        }
    }
}

impl Config {
    pub fn set_max_reservations(mut self, max_reservations: u8) -> Self {
        assert!(max_reservations > 0);
        self.max_reservations = NonZeroU8::new(max_reservations).expect("greater than zero");
        self
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Event {}

impl Behaviour {
    pub fn new_with_config(config: Config) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    fn select_connection_for_reservation(
        &mut self,
        peer_id: PeerId, connection_id: ConnectionId
    ) -> bool {
        if self.pending_target.contains(&(peer_id, connection_id)) {
            return false;
        }

        let info = self
            .connections
            .get_mut(&(peer_id, connection_id))
            .expect("connection is present");

        let addr_with_peer_id = match info.address.clone().with_p2p(peer_id) {
            Ok(addr) => addr,
            Err(addr) => {
                tracing::warn!(%addr, "address unexpectedly contains a different peer id than the connection");
                return false;
            }
        };

        let relay_addr = addr_with_peer_id.with(Protocol::P2pCircuit);

        let opts = ListenOpts::new(relay_addr);

        let id = opts.listener_id();

        info.relay_status = RelayStatus::Supported {
            status: ReservationStatus::Pending { id },
        };
        self.reservations.insert(id, (peer_id, connection_id));
        self.events.push_back(ToSwarm::ListenOn { opts });
        self.pending_target.insert((peer_id, connection_id));

        true
    }

    fn remove_all_reservations(&mut self) {
        let relay_listeners = self
            .reservations
            .iter()
            .map(|(id, (peer_id, conn_id))| (*id, *peer_id, *conn_id))
            .collect::<Vec<_>>();

        for (listener_id, peer_id, connection_id) in relay_listeners {
            let Some(connection) = self.connections.get_mut(&(peer_id, connection_id)) else {
                continue;
            };

            assert!(matches!(
                connection.relay_status,
                RelayStatus::Supported {
                    status: ReservationStatus::Active { id } | ReservationStatus::Pending { id }
                } if id == listener_id
            ));

            connection.relay_status = RelayStatus::Supported {
                status: ReservationStatus::Idle,
            };

            self.events
                .push_back(ToSwarm::RemoveListener { id: listener_id });
        }
    }

    fn disable_reservation(&mut self, id: ListenerId) {
        let Some((peer_id, connection_id)) = self.reservations.remove(&id) else {
            return;
        };

        let Some(connection) = self.connections.get_mut(&(peer_id, connection_id)) else {
            return;
        };

        match connection.relay_status {
            RelayStatus::Supported {
                status: ReservationStatus::Active { .. },
            } => {
                connection.relay_status = RelayStatus::Supported {
                    status: ReservationStatus::Idle,
                };
            }
            RelayStatus::Supported {
                status: ReservationStatus::Pending { .. },
            } => {
                connection.relay_status = RelayStatus::Supported {
                    status: ReservationStatus::Idle,
                };
            }
            RelayStatus::Pending => {
                self.pending_target.remove(&(peer_id, connection_id));
            }
            RelayStatus::Supported {
                status: ReservationStatus::Idle,
            }
            | RelayStatus::NotSupported => {}
        }
    }

    fn meet_reservation_target(&mut self) {
        // check to determine if there is a public external address that could possibly let us know the node
        // is reachable
        if self
            .external_addresses
            .iter()
            .any(|addr| !addr.is_relayed())
        {
            return;
        }

        let max_reservation = self.config.max_reservations.get() as usize;

        let peers_not_supported = self.connections.is_empty()
            || self
                .connections
                .iter()
                .all(|(_, connection)| connection.relay_status == RelayStatus::NotSupported);

        if peers_not_supported {
            return;
        }

        let relayed_targets = self
            .connections
            .iter()
            .filter(|(_, info)| {
                matches!(
                    info.relay_status,
                    RelayStatus::Supported {
                        status: ReservationStatus::Active { .. }
                    }
                )
            })
            .count();

        if relayed_targets == max_reservation {
            return;
        }

        let pending_target_len = self.pending_target.len();

        if pending_target_len >= max_reservation {
            return;
        }

        let possible_targets = self
            .connections
            .iter()
            .filter(|(_, info)| {
                matches!(
                    info.relay_status,
                    RelayStatus::Supported {
                        status: ReservationStatus::Idle
                    }
                )
            })
            .map(|((peer_id, connection_id), _)| (*peer_id, *connection_id))
            .collect::<Vec<_>>();

        let targets_count = std::cmp::min(possible_targets.len(), max_reservation);

        if targets_count == 0 {
            return;
        }

        let remaining_targets_needed = targets_count
            .checked_sub(self.pending_target.len())
            .unwrap_or_default();

        if remaining_targets_needed == 0 {
            return;
        }

        for (peer_id, connection_id) in possible_targets
            .iter()
            .copied()
            .take(remaining_targets_needed)
        {
            if !self.select_connection_for_reservation(peer_id, connection_id) {
                continue;
            }

            if self.pending_target.len() == max_reservation {
                break;
            }
        }

        debug_assert!(self.pending_target.len() <= max_reservation);
    }

    fn active_reservations(&self) -> usize {
        self.connections
            .values()
            .filter(|info| {
                matches!(
                    info.relay_status,
                    RelayStatus::Supported {
                        status: ReservationStatus::Active { .. }
                    }
                )
            })
            .count()
    }

    // fn on_connection_established(
    //     &mut self,
    //     peer_id: PeerId,
    //     connection_id: ConnectionId,
    //     connection_id: ConnectionId,
    //     address: Multiaddr,
    // ) {
    // }
    //
    // fn on_connection_closed(&mut self, peer_id: PeerId, connection_id: ConnectionId) {}
    //
    // fn on_address_change(
    //     &mut self,
    //     peer_id: PeerId,
    //     connection_id: ConnectionId,
    //     old_addr: &Multiaddr,
    //     new_addr: &Multiaddr,
    // ) {
    // }
    //
    // fn on_new_listen_addr(&mut self, listener_id: ListenerId, addr: &Multiaddr) {}
    // fn on_expired_listen_addr(&mut self, listener_id: ListenerId) {}
    // fn on_listener_error(&mut self, listener_id: ListenerId, error: &std::io::Error) {}
    // fn on_listener_closed(&mut self, listener_id: ListenerId) {}
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Either<handler::Handler, dummy::ConnectionHandler>;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if local_addr.is_relayed() {
            Ok(Either::Right(dummy::ConnectionHandler))
        } else {
            Ok(Either::Left(handler::Handler::default()))
        }
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if addr.is_relayed() {
            Ok(Either::Right(dummy::ConnectionHandler))
        } else {
            Ok(Either::Left(handler::Handler::default()))
        }
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        let change = self.external_addresses.on_swarm_event(&event);

        if change
            && self
                .external_addresses
                .iter()
                .any(|addr| !addr.is_relayed())
        {
            self.remove_all_reservations();
            return;
        }

        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                endpoint,
                connection_id,
                ..
            }) => {
                let remote_addr = endpoint.get_remote_address().clone();

                let mut connection = Connection {
                    address: remote_addr,
                    relay_status: RelayStatus::Pending,
                };

                connection.disqualify_connection_if_relayed();

                self.connections
                    .insert((peer_id, connection_id), connection);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                let connection = self
                    .connections
                    .remove(&(peer_id, connection_id))
                    .expect("valid connection");

                let id = match connection.relay_status {
                    RelayStatus::Supported {
                        status: ReservationStatus::Active { id },
                    } => id,
                    RelayStatus::Supported {
                        status: ReservationStatus::Pending { id },
                    } => id,
                    _ => return,
                };

                self.reservations.remove(&id);
                self.pending_target.remove(&(peer_id, connection_id));

                let max_reservation = self.config.max_reservations.get() as usize;

                let active_reservations = self.active_reservations();

                if active_reservations < max_reservation {
                    self.meet_reservation_target();
                }
            }
            FromSwarm::AddressChange(AddressChange {
                peer_id,
                connection_id,
                old,
                new,
            }) => {
                let connection = self
                    .connections
                    .get_mut(&(peer_id, connection_id))
                    .expect("valid connection");

                let old_addr = old.get_remote_address();
                let new_addr = new.get_remote_address();

                debug_assert!(old_addr != new_addr);

                connection.address = new_addr.clone();
            }
            FromSwarm::NewListenAddr(NewListenAddr { listener_id, addr }) => {
                // we only care about any new relayed address
                if !addr.iter().any(|protocol| protocol == Protocol::P2pCircuit) {
                    return;
                }

                let Some((peer_id, connection_id)) = self.reservations.get(&listener_id).copied()
                else {
                    return;
                };

                let connection = self
                    .connections
                    .get_mut(&(peer_id, connection_id))
                    .expect("valid connection");

                let RelayStatus::Supported {
                    status: ReservationStatus::Pending { id },
                } = connection.relay_status
                else {
                    return;
                };

                connection.relay_status = RelayStatus::Supported {
                    status: ReservationStatus::Active { id },
                };
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr { listener_id, .. }) => {
                self.disable_reservation(listener_id);
            }
            FromSwarm::ListenerError(ListenerError { listener_id, .. }) => {
                self.disable_reservation(listener_id);
            }
            FromSwarm::ListenerClosed(ListenerClosed { listener_id, .. }) => {
                self.disable_reservation(listener_id);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        let Either::Left(event) = event;

        let connection = self
            .connections
            .get_mut(&(peer_id, connection_id))
            .expect("valid connection");

        match event {
            Out::Supported => {
                connection.relay_status = RelayStatus::Supported {
                    status: ReservationStatus::Idle,
                };
                self.meet_reservation_target();
            }
            Out::Unsupported => {
                let _previous_status = connection.relay_status;
                connection.relay_status = RelayStatus::NotSupported;
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }
        self.waker.replace(cx.waker().clone());
        Poll::Pending
    }
}
