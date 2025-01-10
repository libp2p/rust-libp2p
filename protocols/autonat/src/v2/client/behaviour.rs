use std::{
    collections::{HashMap, VecDeque},
    fmt::{Debug, Display, Formatter},
    task::{Context, Poll},
    time::Duration,
};

use either::Either;
use futures::FutureExt;
use futures_timer::Delay;
use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::ConnectionEstablished, ConnectionClosed, ConnectionDenied, ConnectionHandler,
    ConnectionId, FromSwarm, NetworkBehaviour, NewExternalAddrCandidate, NotifyHandler, ToSwarm,
};
use rand::prelude::*;
use rand_core::OsRng;

use super::handler::{
    dial_back::{self, IncomingNonce},
    dial_request,
};
use crate::v2::{protocol::DialRequest, Nonce};

#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// How many candidates we will test at most.
    pub(crate) max_candidates: usize,

    /// The interval at which we will attempt to confirm candidates as external addresses.
    pub(crate) probe_interval: Duration,
}

impl Config {
    pub fn with_max_candidates(self, max_candidates: usize) -> Self {
        Self {
            max_candidates,
            ..self
        }
    }

    pub fn with_probe_interval(self, probe_interval: Duration) -> Self {
        Self {
            probe_interval,
            ..self
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_candidates: 10,
            probe_interval: Duration::from_secs(5),
        }
    }
}

pub struct Behaviour<R = OsRng>
where
    R: RngCore + 'static,
{
    rng: R,
    config: Config,
    pending_events: VecDeque<
        ToSwarm<
            <Self as NetworkBehaviour>::ToSwarm,
            <<Self as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::FromBehaviour,
        >,
    >,
    address_candidates: HashMap<Multiaddr, AddressInfo>,
    next_tick: Delay,
    peer_info: HashMap<ConnectionId, ConnectionInfo>,
}

impl<R> NetworkBehaviour for Behaviour<R>
where
    R: RngCore + 'static,
{
    type ConnectionHandler = Either<dial_request::Handler, dial_back::Handler>;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        Ok(Either::Right(dial_back::Handler::new()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        Ok(Either::Left(dial_request::Handler::new()))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::NewExternalAddrCandidate(NewExternalAddrCandidate { addr }) => {
                self.address_candidates
                    .entry(addr.clone())
                    .or_default()
                    .score += 1;
            }
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint: _,
                ..
            }) => {
                self.peer_info.insert(
                    connection_id,
                    ConnectionInfo {
                        peer_id,
                        supports_autonat: false,
                    },
                );
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                let info = self
                    .peer_info
                    .remove(&connection_id)
                    .expect("inconsistent state");

                if info.supports_autonat {
                    tracing::debug!(%peer_id, "Disconnected from AutoNAT server");
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: <Self::ConnectionHandler as ConnectionHandler>::ToBehaviour,
    ) {
        let (nonce, outcome) = match event {
            Either::Right(IncomingNonce { nonce, sender }) => {
                let Some((_, info)) = self
                    .address_candidates
                    .iter_mut()
                    .find(|(_, info)| info.is_pending_with_nonce(nonce))
                else {
                    let _ = sender.send(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Received unexpected nonce: {nonce} from {peer_id}"),
                    )));
                    return;
                };

                info.status = TestStatus::Received(nonce);
                tracing::debug!(%peer_id, %nonce, "Successful dial-back");

                let _ = sender.send(Ok(()));

                return;
            }
            Either::Left(dial_request::ToBehaviour::PeerHasServerSupport) => {
                self.peer_info
                    .get_mut(&connection_id)
                    .expect("inconsistent state")
                    .supports_autonat = true;
                return;
            }
            Either::Left(dial_request::ToBehaviour::TestOutcome { nonce, outcome }) => {
                (nonce, outcome)
            }
        };

        let ((tested_addr, bytes_sent), result) = match outcome {
            Ok(address) => {
                let received_dial_back = self
                    .address_candidates
                    .iter_mut()
                    .any(|(_, info)| info.is_received_with_nonce(nonce));

                if !received_dial_back {
                    tracing::warn!(
                        %peer_id,
                        %nonce,
                        "Server reported reachbility but we never received a dial-back"
                    );
                    return;
                }

                self.pending_events
                    .push_back(ToSwarm::ExternalAddrConfirmed(address.0.clone()));

                (address, Ok(()))
            }
            Err(dial_request::Error::UnsupportedProtocol) => {
                self.peer_info
                    .get_mut(&connection_id)
                    .expect("inconsistent state")
                    .supports_autonat = false;

                self.reset_status_to(nonce, TestStatus::Untested); // Reset so it will be tried again.

                return;
            }
            Err(dial_request::Error::Io(e)) => {
                tracing::debug!(
                    %peer_id,
                    %nonce,
                    "Failed to complete AutoNAT probe: {e}"
                );

                self.reset_status_to(nonce, TestStatus::Untested); // Reset so it will be tried again.

                return;
            }
            Err(dial_request::Error::AddressNotReachable {
                address,
                bytes_sent,
                error,
            }) => {
                self.reset_status_to(nonce, TestStatus::Failed);

                ((address, bytes_sent), Err(error))
            }
        };

        self.pending_events.push_back(ToSwarm::GenerateEvent(Event {
            tested_addr,
            bytes_sent,
            server: peer_id,
            result: result.map_err(|e| Error { inner: e }),
        }));
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, <Self::ConnectionHandler as ConnectionHandler>::FromBehaviour>>
    {
        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(event);
            }

            if self.next_tick.poll_unpin(cx).is_ready() {
                self.next_tick.reset(self.config.probe_interval);

                self.issue_dial_requests_for_untested_candidates();
                continue;
            }

            return Poll::Pending;
        }
    }
}

impl<R> Behaviour<R>
where
    R: RngCore + 'static,
{
    pub fn new(rng: R, config: Config) -> Self {
        Self {
            rng,
            next_tick: Delay::new(config.probe_interval),
            config,
            pending_events: VecDeque::new(),
            address_candidates: HashMap::new(),
            peer_info: HashMap::new(),
        }
    }

    /// Issues dial requests to random AutoNAT servers for the most frequently reported, untested
    /// candidates.
    ///
    /// In the current implementation, we only send a single address to each AutoNAT server.
    /// This spreads our candidates out across all servers we are connected to which should give us
    /// pretty fast feedback on all of them.
    fn issue_dial_requests_for_untested_candidates(&mut self) {
        for addr in self.untested_candidates() {
            let Some((conn_id, peer_id)) = self.random_autonat_server() else {
                tracing::debug!("Not connected to any AutoNAT servers");
                return;
            };

            let nonce = self.rng.gen();
            self.address_candidates
                .get_mut(&addr)
                .expect("only emit candidates")
                .status = TestStatus::Pending(nonce);

            self.pending_events.push_back(ToSwarm::NotifyHandler {
                peer_id,
                handler: NotifyHandler::One(conn_id),
                event: Either::Left(DialRequest {
                    nonce,
                    addrs: vec![addr],
                }),
            });
        }
    }

    /// Returns all untested candidates, sorted by the frequency they were reported at.
    ///
    /// More frequently reported candidates are considered to more likely be external addresses and
    /// thus tested first.
    fn untested_candidates(&self) -> impl Iterator<Item = Multiaddr> {
        let mut entries = self
            .address_candidates
            .iter()
            .filter(|(_, info)| info.status == TestStatus::Untested)
            .map(|(addr, count)| (addr.clone(), *count))
            .collect::<Vec<_>>();

        entries.sort_unstable_by_key(|(_, info)| info.score);

        if entries.is_empty() {
            tracing::debug!("No untested address candidates");
        }

        entries
            .into_iter()
            .rev() // `sort_unstable` is ascending
            .take(self.config.max_candidates)
            .map(|(addr, _)| addr)
    }

    /// Chooses an active connection to one of our peers that reported support for the
    /// [`DIAL_REQUEST_PROTOCOL`](crate::v2::DIAL_REQUEST_PROTOCOL) protocol.
    fn random_autonat_server(&mut self) -> Option<(ConnectionId, PeerId)> {
        let (conn_id, info) = self
            .peer_info
            .iter()
            .filter(|(_, info)| info.supports_autonat)
            .choose(&mut self.rng)?;

        Some((*conn_id, info.peer_id))
    }

    fn reset_status_to(&mut self, nonce: Nonce, new_status: TestStatus) {
        let Some((_, info)) = self
            .address_candidates
            .iter_mut()
            .find(|(_, i)| i.is_pending_with_nonce(nonce) || i.is_received_with_nonce(nonce))
        else {
            return;
        };

        info.status = new_status;
    }

    // FIXME: We don't want test-only APIs in our public API.
    #[doc(hidden)]
    pub fn validate_addr(&mut self, addr: &Multiaddr) {
        if let Some(info) = self.address_candidates.get_mut(addr) {
            info.status = TestStatus::Received(self.rng.next_u64());
        }
    }
}

impl Default for Behaviour<OsRng> {
    fn default() -> Self {
        Self::new(OsRng, Config::default())
    }
}

pub struct Error {
    pub(crate) inner: dial_request::DialBackError,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.inner, f)
    }
}

#[derive(Debug)]
pub struct Event {
    /// The address that was selected for testing.
    pub tested_addr: Multiaddr,
    /// The amount of data that was sent to the server.
    /// Is 0 if it wasn't necessary to send any data.
    /// Otherwise it's a number between 30.000 and 100.000.
    pub bytes_sent: usize,
    /// The peer id of the server that was selected for testing.
    pub server: PeerId,
    /// The result of the test. If the test was successful, this is `Ok(())`.
    /// Otherwise it's an error.
    pub result: Result<(), Error>,
}

struct ConnectionInfo {
    peer_id: PeerId,
    supports_autonat: bool,
}

#[derive(Copy, Clone, Default)]
struct AddressInfo {
    score: usize,
    status: TestStatus,
}

impl AddressInfo {
    fn is_pending_with_nonce(&self, nonce: Nonce) -> bool {
        match self.status {
            TestStatus::Pending(c) => c == nonce,
            _ => false,
        }
    }

    fn is_received_with_nonce(&self, nonce: Nonce) -> bool {
        match self.status {
            TestStatus::Received(c) => c == nonce,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
enum TestStatus {
    #[default]
    Untested,
    Pending(Nonce),
    Failed,
    Received(Nonce),
}
