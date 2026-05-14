use std::{
    collections::{HashMap, HashSet, VecDeque},
    num::NonZeroU8,
    task::{Context, Poll, Waker},
};

use either::Either;
use libp2p_core::{
    Endpoint,
    multiaddr::Protocol,
    transport::{ListenerId, PortUse},
};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    ExternalAddresses, ListenOpts, NewListenAddr,
    derive_prelude::{
        AddressChange, ConnectionClosed, ConnectionDenied, ConnectionEstablished, ConnectionId,
        ExpiredListenAddr, ExternalAddrConfirmed, FromSwarm, ListenerClosed, ListenerError,
        Multiaddr, NetworkBehaviour, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    dummy,
};

use crate::{autorelay::handler::Out, multiaddr_ext::MultiaddrExt};

mod handler;

#[derive(Default, Debug)]
pub struct Behaviour {
    config: Config,
    external_addresses: ExternalAddresses,
    events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,

    connections: HashMap<(PeerId, ConnectionId), Connection>,

    reservations: HashMap<ListenerId, (PeerId, ConnectionId)>,

    external_reservations: HashMap<ListenerId, PeerId>,

    waker: Option<Waker>,
}

#[derive(Debug)]
struct Connection {
    address: Multiaddr,
    relay_status: RelayStatus,
}

impl Connection {
    /// Mark relayed connection as not supported
    pub(crate) fn disqualify_connection_if_relayed(&mut self)  {
        if self.address.is_relayed() {
            self.relay_status = RelayStatus::NotSupported;
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

    fn select_connection_for_reservation(&mut self, peer_id: PeerId, connection_id: ConnectionId) {
        let info = self
            .connections
            .get_mut(&(peer_id, connection_id))
            .expect("connection is present");

        if info.relay_status
            != (RelayStatus::Supported {
                status: ReservationStatus::Idle,
            })
        {
            return;
        }

        let addr_with_peer_id = match info.address.clone().with_p2p(peer_id) {
            Ok(addr) => addr,
            Err(addr) => {
                tracing::warn!(%addr, "address unexpectedly contains a different peer id than the connection");
                return;
            }
        };

        let opts = ListenOpts::new(addr_with_peer_id.with(Protocol::P2pCircuit));
        let id = opts.listener_id();

        info.relay_status = RelayStatus::Supported {
            status: ReservationStatus::Pending { id },
        };
        self.reservations.insert(id, (peer_id, connection_id));
        self.events.push_back(ToSwarm::ListenOn { opts });
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

            if !matches!(
                connection.relay_status,
                RelayStatus::Supported {
                    status: ReservationStatus::Active { id } | ReservationStatus::Pending { id }
                } if id == listener_id
            ) {
                continue;
            }

            connection.relay_status = RelayStatus::Supported {
                status: ReservationStatus::Idle,
            };

            self.events
                .push_back(ToSwarm::RemoveListener { id: listener_id });
        }
    }

    fn disable_reservation(&mut self, id: ListenerId) {
        if self.external_reservations.remove(&id).is_some() {
            self.meet_reservation_target();
            return;
        }

        let Some((peer_id, connection_id)) = self.reservations.remove(&id) else {
            return;
        };

        if let Some(connection) = self.connections.get_mut(&(peer_id, connection_id))
            && matches!(
                connection.relay_status,
                RelayStatus::Supported {
                    status: ReservationStatus::Active { .. } | ReservationStatus::Pending { .. }
                }
            )
        {
            connection.relay_status = RelayStatus::Supported {
                status: ReservationStatus::Idle,
            };
        }

        self.meet_reservation_target();
    }

    fn covered_peers(&self) -> HashSet<PeerId> {
        self.reservations
            .values()
            .map(|(peer_id, _)| *peer_id)
            .chain(self.external_reservations.values().copied())
            .collect()
    }

    fn meet_reservation_target(&mut self) {
        if self
            .external_addresses
            .iter()
            .any(|addr| !addr.is_relayed())
        {
            return;
        }

        let max = self.config.max_reservations.get() as usize;
        let covered = self.covered_peers();
        let budget = max.saturating_sub(covered.len());
        if budget == 0 {
            return;
        }

        let mut candidates = HashMap::new();
        for ((peer_id, connection_id), info) in self.connections.iter() {
            if covered.contains(peer_id) {
                continue;
            }
            if info.relay_status
                == (RelayStatus::Supported {
                    status: ReservationStatus::Idle,
                })
            {
                candidates.entry(*peer_id).or_insert(*connection_id);
            }
        }

        for (peer_id, connection_id) in candidates.into_iter().take(budget) {
            self.select_connection_for_reservation(peer_id, connection_id);
        }

        debug_assert!(self.covered_peers().len() <= max);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
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
        self.external_addresses.on_swarm_event(&event);

        if let FromSwarm::ExternalAddrConfirmed(ExternalAddrConfirmed { addr }) = &event
            && !addr.is_relayed()
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

                if let RelayStatus::Supported {
                    status: ReservationStatus::Active { id } | ReservationStatus::Pending { id },
                } = connection.relay_status
                {
                    self.reservations.remove(&id);
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
                if !addr.is_relayed() {
                    return;
                }

                if let Some((peer_id, connection_id)) =
                    self.reservations.get(&listener_id).copied()
                {
                    let connection = self
                        .connections
                        .get_mut(&(peer_id, connection_id))
                        .expect("valid connection");

                    if matches!(
                        connection.relay_status,
                        RelayStatus::Supported {
                            status: ReservationStatus::Pending { id }
                        } if id == listener_id
                    ) {
                        connection.relay_status = RelayStatus::Supported {
                            status: ReservationStatus::Active { id: listener_id },
                        };
                    }
                    return;
                }

                if let Some(relay_peer_id) = addr.relay_peer_id() {
                    self.external_reservations
                        .insert(listener_id, relay_peer_id);
                }
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
                if matches!(
                    connection.relay_status,
                    RelayStatus::Pending | RelayStatus::NotSupported
                ) {
                    connection.relay_status = RelayStatus::Supported {
                        status: ReservationStatus::Idle,
                    };
                    self.meet_reservation_target();
                }
            }
            Out::Unsupported => {
                let drop_listener = match connection.relay_status {
                    RelayStatus::Supported {
                        status: ReservationStatus::Pending { id } | ReservationStatus::Active { id },
                    } => Some(id),
                    _ => None,
                };
                connection.relay_status = RelayStatus::NotSupported;
                if let Some(id) = drop_listener {
                    self.reservations.remove(&id);
                    self.events.push_back(ToSwarm::RemoveListener { id });
                    self.meet_reservation_target();
                }
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
