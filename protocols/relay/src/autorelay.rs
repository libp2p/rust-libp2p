use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    num::NonZeroU8,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use either::Either;
use libp2p_core::{
    Endpoint,
    multiaddr::Protocol,
    transport::{ListenerId, PortUse},
};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    ExternalAddresses, ListenOpts, NewListenAddr, NotifyHandler,
    derive_prelude::{
        AddressChange, ConnectionClosed, ConnectionDenied, ConnectionEstablished, ConnectionId,
        DialFailure, ExpiredListenAddr, FromSwarm, ListenerClosed, ListenerError, Multiaddr,
        NetworkBehaviour, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    dial_opts::DialOpts,
    dummy,
};

use crate::{autorelay::handler::Out, multiaddr_ext::MultiaddrExt};

mod handler;

#[derive(Debug)]
pub struct Behaviour {
    config: Config,
    status: Status,
    auto_status_change: bool,
    external_addresses: ExternalAddresses,
    events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,

    connections: HashMap<(PeerId, ConnectionId), Connection>,

    reservations: HashMap<ListenerId, (PeerId, ConnectionId)>,

    external_reservations: HashMap<ListenerId, PeerId>,

    static_relays: HashMap<PeerId, Vec<Multiaddr>>,

    static_dial_cooldowns: HashMap<PeerId, Instant>,

    failure_counts: HashMap<PeerId, u32>,

    previous_relays: VecDeque<(PeerId, Multiaddr, Instant)>,

    relays_available: bool,

    waker: Option<Waker>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            config: Config::default(),
            status: Status::Enable,
            auto_status_change: true,
            external_addresses: ExternalAddresses::default(),
            events: VecDeque::new(),
            connections: HashMap::new(),
            reservations: HashMap::new(),
            external_reservations: HashMap::new(),
            static_relays: HashMap::new(),
            static_dial_cooldowns: HashMap::new(),
            failure_counts: HashMap::new(),
            previous_relays: VecDeque::new(),
            relays_available: false,
            waker: None,
        }
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    #[default]
    Enable,
    Disable,
}

#[derive(Debug)]
struct Connection {
    address: Multiaddr,
    relay_status: RelayStatus,
}

impl Connection {
    /// Mark relayed connection as not supported
    pub(crate) fn disqualify_connection_if_relayed(&mut self) {
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
    Blacklisted,
}

#[derive(Debug)]
pub struct Config {
    max_reservations: NonZeroU8,
    failure_cooldown: Duration,
    failure_cooldown_max: Duration,
    max_previous_relays: usize,
    static_relays: HashMap<PeerId, Vec<Multiaddr>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_reservations: NonZeroU8::new(2).unwrap(),
            failure_cooldown: Duration::from_secs(30),
            failure_cooldown_max: Duration::from_secs(10 * 60),
            max_previous_relays: 16,
            static_relays: HashMap::new(),
        }
    }
}

impl Config {
    pub fn set_max_reservations(mut self, max_reservations: NonZeroU8) -> Self {
        self.max_reservations = max_reservations;
        self
    }

    pub fn set_failure_cooldown(mut self, duration: Duration) -> Self {
        self.failure_cooldown = duration;
        self
    }

    pub fn set_failure_cooldown_max(mut self, duration: Duration) -> Self {
        self.failure_cooldown_max = duration;
        self
    }

    pub fn set_max_previous_relays(mut self, max: usize) -> Self {
        self.max_previous_relays = max;
        self
    }

    pub fn add_static_relay(mut self, peer_id: PeerId, addresses: Vec<Multiaddr>) -> Self {
        let entry = self.static_relays.entry(peer_id).or_default();
        for addr in addresses {
            if !entry.contains(&addr) {
                entry.push(addr);
            }
        }
        self
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Event {
    /// The status of the local node has changed.
    StatusChanged { status: Status },
    /// No connected peer supports the HOP protocol.
    NoRelaysAvailable,
    /// At least one connected peer supports the HOP protocol.
    RelaysAvailable,
}

impl Behaviour {
    pub fn new_with_config(mut config: Config) -> Self {
        let initial_static_relays = std::mem::take(&mut config.static_relays);
        let mut behaviour = Self {
            config,
            ..Default::default()
        };
        for (peer_id, addresses) in initial_static_relays {
            for address in addresses {
                behaviour.add_static_relay(peer_id, address);
            }
        }
        behaviour
    }

    /// Sets the autorelay status.
    pub fn set_status(&mut self, status: Option<Status>) {
        match status {
            Some(status) => {
                self.auto_status_change = false;
                if self.status != status {
                    self.status = status;
                    self.events
                        .push_back(ToSwarm::GenerateEvent(Event::StatusChanged { status }));
                    if status == Status::Enable {
                        self.meet_reservation_target();
                    }
                }
            }
            None => {
                self.auto_status_change = true;
                self.determine_status_from_external_addresses();
            }
        }

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    /// Register a peer as a static relay.
    ///
    /// This will dial and establish a connection to the peer if it doesn't already have a direct
    /// connection.
    /// Note that peers that are through a relay cannot be used as a static peer
    pub fn add_static_relay(&mut self, peer_id: PeerId, address: Multiaddr) {
        if address.is_relayed() {
            tracing::warn!(%peer_id, %address, "static relay address is relayed. ignoring.");
            return;
        }

        let entry = self.static_relays.entry(peer_id).or_default();
        if entry.contains(&address) {
            tracing::warn!(%peer_id, %address, "static relay address already exist");
        } else {
            entry.push(address);
        }
        let combined = entry.clone();

        if self.is_peer_idle(&peer_id) {
            self.evict_for_static_peer(peer_id);
        }

        if !self.queue_static_dial(peer_id, combined) {
            self.meet_reservation_target();
        }

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    /// Remove peer as a static relay.
    /// This will not close any connections or terminate any existing reservation with the relay
    pub fn remove_static_relay(&mut self, peer_id: &PeerId) -> bool {
        self.static_dial_cooldowns.remove(peer_id);
        self.static_relays.remove(peer_id).is_some()
    }

    pub fn static_relays(&self) -> impl Iterator<Item = (&PeerId, &[Multiaddr])> {
        self.static_relays
            .iter()
            .map(|(peer, addrs)| (peer, addrs.as_slice()))
    }

    pub fn previous_relays(&self) -> impl Iterator<Item = (&PeerId, &Multiaddr, &Instant)> {
        self.previous_relays
            .iter()
            .map(|(peer, addr, ts)| (peer, addr, ts))
    }

    fn static_dial_in_cooldown(&self, peer_id: &PeerId) -> bool {
        self.static_dial_cooldowns
            .get(peer_id)
            .is_some_and(|deadline| *deadline > Instant::now())
    }

    fn queue_static_dial(&mut self, peer_id: PeerId, addresses: Vec<Multiaddr>) -> bool {
        if addresses.is_empty()
            || self.has_direct_connection(&peer_id)
            || self.static_dial_in_cooldown(&peer_id)
        {
            return false;
        }
        let opts = DialOpts::peer_id(peer_id).addresses(addresses).build();
        self.events.push_back(ToSwarm::Dial { opts });
        true
    }

    fn record_previous_relay(&mut self, peer_id: PeerId, address: Multiaddr) {
        let max = self.config.max_previous_relays;
        if max == 0 {
            return;
        }
        self.previous_relays.retain(|(p, _, _)| *p != peer_id);
        if self.previous_relays.len() >= max {
            self.previous_relays.pop_front();
        }
        self.previous_relays
            .push_back((peer_id, address, Instant::now()));
    }

    fn forget_previous_relay(&mut self, peer_id: &PeerId) {
        self.previous_relays.retain(|(p, _, _)| p != peer_id);
    }

    fn record_failure(&mut self, peer_id: PeerId) -> Duration {
        let attempts = self.failure_counts.entry(peer_id).or_insert(0);
        *attempts = attempts.saturating_add(1);
        let exponent = attempts.saturating_sub(1).min(20);
        let scale = 1u32 << exponent;
        self.config
            .failure_cooldown
            .saturating_mul(scale)
            .min(self.config.failure_cooldown_max)
    }

    fn clear_failure(&mut self, peer_id: &PeerId) {
        self.failure_counts.remove(peer_id);
    }

    fn determine_status_from_external_addresses(&mut self) {
        let has_public_addr = self
            .external_addresses
            .iter()
            .any(|addr| !addr.is_relayed());

        let new_status = match has_public_addr {
            true => Status::Disable,
            false => Status::Enable,
        };
        if new_status != self.status {
            self.status = new_status;
            self.events
                .push_back(ToSwarm::GenerateEvent(Event::StatusChanged {
                    status: new_status,
                }));
            match new_status {
                Status::Enable => self.meet_reservation_target(),
                Status::Disable => self.remove_all_reservations(),
            }
        }
    }

    fn is_peer_idle(&self, peer_id: &PeerId) -> bool {
        self.connections.iter().any(|((pid, _), info)| {
            pid == peer_id
                && info.relay_status
                    == RelayStatus::Supported {
                        status: ReservationStatus::Idle,
                    }
        })
    }

    fn has_direct_connection(&self, peer_id: &PeerId) -> bool {
        self.connections
            .iter()
            .any(|((pid, _), info)| pid == peer_id && !info.address.is_relayed())
    }

    fn evict_for_static_peer(&mut self, new_static: PeerId) {
        let covered = self.covered_peers();
        if covered.contains(&new_static) {
            return;
        }
        let max = self.config.max_reservations.get() as usize;
        if covered.len() < max {
            return;
        }

        if let Some(listener_id) = self
            .reservations
            .iter()
            .find(|(_, (peer_id, _))| !self.static_relays.contains_key(peer_id))
            .map(|(listener_id, _)| *listener_id)
        {
            self.events
                .push_back(ToSwarm::RemoveListener { id: listener_id });
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
                tracing::warn!(%addr, "address unexpectedly contains a different peer id than the connection; marking relay connection ineligible");
                info.relay_status = RelayStatus::NotSupported;
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

    /// Removes all existing reservations.
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

    fn disable_reservation(&mut self, id: ListenerId, failed: bool) {
        if self.external_reservations.remove(&id).is_some() {
            self.meet_reservation_target();
            return;
        }

        let Some((peer_id, connection_id)) = self.reservations.remove(&id) else {
            return;
        };

        let Some(address) = self
            .connections
            .get(&(peer_id, connection_id))
            .filter(|info| {
                matches!(
                    info.relay_status,
                    RelayStatus::Supported {
                        status: ReservationStatus::Active { .. }
                            | ReservationStatus::Pending { .. }
                    }
                )
            })
            .map(|info| info.address.clone())
        else {
            self.meet_reservation_target();
            return;
        };

        let blacklist_duration = failed.then(|| self.record_failure(peer_id));

        let connection = self
            .connections
            .get_mut(&(peer_id, connection_id))
            .expect("connection is tracked");
        match blacklist_duration {
            Some(duration) => {
                connection.relay_status = RelayStatus::Supported {
                    status: ReservationStatus::Blacklisted,
                };
                self.events.push_back(ToSwarm::NotifyHandler {
                    peer_id,
                    handler: NotifyHandler::One(connection_id),
                    event: Either::Left(handler::In::Blacklist { duration }),
                });
            }
            None => {
                connection.relay_status = RelayStatus::Supported {
                    status: ReservationStatus::Idle,
                };
            }
        }

        self.record_previous_relay(peer_id, address);
        self.meet_reservation_target();
    }

    fn covered_peers(&self) -> HashSet<PeerId> {
        self.reservations
            .values()
            .map(|(peer_id, _)| *peer_id)
            .chain(self.external_reservations.values().copied())
            .collect()
    }

    /// Meet the reservation target by selecting connections to establish a reservation.
    fn meet_reservation_target(&mut self) {
        if self.status == Status::Disable {
            return;
        }

        let max = self.config.max_reservations.get() as usize;
        let covered = self.covered_peers();
        let budget = max.saturating_sub(covered.len());
        if budget == 0 {
            return;
        }

        let mut static_candidates = BTreeMap::new();
        let mut candidates: BTreeMap<_, ConnectionId> = BTreeMap::new();
        for ((peer_id, connection_id), info) in self.connections.iter() {
            if covered.contains(peer_id) {
                continue;
            }
            if info.relay_status
                != (RelayStatus::Supported {
                    status: ReservationStatus::Idle,
                })
            {
                continue;
            }
            let bucket = if self.static_relays.contains_key(peer_id) {
                &mut static_candidates
            } else {
                &mut candidates
            };
            bucket
                .entry(*peer_id)
                .and_modify(|existing| *existing = (*existing).min(*connection_id))
                .or_insert(*connection_id);
        }

        let selected_candidates: Vec<(PeerId, ConnectionId)> = static_candidates
            .into_iter()
            .chain(candidates)
            .take(budget)
            .collect();

        for (peer_id, connection_id) in selected_candidates {
            self.select_connection_for_reservation(peer_id, connection_id);
        }

        debug_assert!(self.covered_peers().len() <= max);
    }

    fn update_relay_availability(&mut self) {
        let has_hop_peer = self
            .connections
            .values()
            .any(|info| matches!(info.relay_status, RelayStatus::Supported { .. }));

        match (has_hop_peer, self.relays_available) {
            (true, false) => {
                self.relays_available = true;
                self.events
                    .push_back(ToSwarm::GenerateEvent(Event::RelaysAvailable));
            }
            (false, true) => {
                self.relays_available = false;
                self.events
                    .push_back(ToSwarm::GenerateEvent(Event::NoRelaysAvailable));
            }
            _ => {}
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
        let change = self.external_addresses.on_swarm_event(&event);

        if self.auto_status_change && change {
            self.determine_status_from_external_addresses();
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

                if self.static_relays.contains_key(&peer_id) {
                    self.static_dial_cooldowns.remove(&peer_id);
                }
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                let Some(connection) = self.connections.remove(&(peer_id, connection_id)) else {
                    return;
                };

                if !self.connections.keys().any(|(pid, _)| *pid == peer_id) {
                    self.clear_failure(&peer_id);
                }

                let had_reservation = matches!(
                    connection.relay_status,
                    RelayStatus::Supported {
                        status: ReservationStatus::Active { .. }
                            | ReservationStatus::Pending { .. }
                            | ReservationStatus::Blacklisted
                    }
                );

                if let RelayStatus::Supported {
                    status: ReservationStatus::Active { id } | ReservationStatus::Pending { id },
                } = connection.relay_status
                {
                    self.reservations.remove(&id);
                    self.meet_reservation_target();
                }

                if had_reservation {
                    self.record_previous_relay(peer_id, connection.address);
                }

                if let Some(addresses) = self.static_relays.get(&peer_id).cloned() {
                    self.queue_static_dial(peer_id, addresses);
                }

                self.update_relay_availability();
            }
            FromSwarm::AddressChange(AddressChange {
                peer_id,
                connection_id,
                old: _,
                new,
            }) => {
                let Some(connection) = self.connections.get_mut(&(peer_id, connection_id)) else {
                    return;
                };

                let new_addr = new.get_remote_address();

                connection.address = new_addr.clone();
            }
            FromSwarm::NewListenAddr(NewListenAddr { listener_id, addr }) => {
                if !addr.is_relayed() {
                    return;
                }

                if let Some((peer_id, connection_id)) = self.reservations.get(&listener_id).copied()
                {
                    let Some(connection) = self.connections.get_mut(&(peer_id, connection_id))
                    else {
                        return;
                    };

                    if matches!(
                        connection.relay_status,
                        RelayStatus::Supported {
                            status: ReservationStatus::Pending { id }
                        } if id == listener_id
                    ) {
                        connection.relay_status = RelayStatus::Supported {
                            status: ReservationStatus::Active { id: listener_id },
                        };
                        self.forget_previous_relay(&peer_id);
                        self.clear_failure(&peer_id);
                    }
                    return;
                }

                if let Some(relay_peer_id) = addr.relay_peer_id() {
                    self.external_reservations
                        .insert(listener_id, relay_peer_id);
                }
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr { listener_id, .. }) => {
                self.disable_reservation(listener_id, false);
            }
            FromSwarm::ListenerError(ListenerError { listener_id, .. }) => {
                self.disable_reservation(listener_id, true);
            }
            FromSwarm::ListenerClosed(ListenerClosed {
                listener_id,
                reason,
                ..
            }) => {
                self.disable_reservation(listener_id, reason.is_err());
            }
            FromSwarm::DialFailure(DialFailure {
                peer_id: Some(peer_id),
                error,
                ..
            }) if self.static_relays.contains_key(&peer_id) => {
                tracing::warn!(%peer_id, %error, "dial to static relay failed");
                self.static_dial_cooldowns
                    .insert(peer_id, Instant::now() + self.config.failure_cooldown);
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

        let Some(connection) = self.connections.get_mut(&(peer_id, connection_id)) else {
            return;
        };

        match event {
            Out::Supported => {
                if matches!(
                    connection.relay_status,
                    RelayStatus::Pending | RelayStatus::NotSupported
                ) {
                    connection.relay_status = RelayStatus::Supported {
                        status: ReservationStatus::Idle,
                    };
                    if self.static_relays.contains_key(&peer_id) {
                        self.evict_for_static_peer(peer_id);
                    }
                    self.meet_reservation_target();
                    self.update_relay_availability();
                }
            }
            Out::Unsupported => {
                let drop_listener = match connection.relay_status {
                    RelayStatus::Supported {
                        status: ReservationStatus::Pending { id } | ReservationStatus::Active { id },
                    } => Some(id),
                    _ => None,
                };
                let lost_address = drop_listener.map(|_| connection.address.clone());
                connection.relay_status = RelayStatus::NotSupported;
                if let Some(id) = drop_listener {
                    self.reservations.remove(&id);
                    self.events.push_back(ToSwarm::RemoveListener { id });
                    self.meet_reservation_target();
                }
                if let Some(address) = lost_address {
                    self.record_previous_relay(peer_id, address);
                }
                self.update_relay_availability();
            }
            Out::BlacklistExpired => {
                if matches!(
                    connection.relay_status,
                    RelayStatus::Supported {
                        status: ReservationStatus::Blacklisted
                    }
                ) {
                    connection.relay_status = RelayStatus::Supported {
                        status: ReservationStatus::Idle,
                    };
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

        self.waker = Some(cx.waker().clone());

        Poll::Pending
    }
}
