use std::{
    collections::{HashMap, VecDeque},
    io,
    task::{Context, Poll},
};

use either::Either;
use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dial_opts::{DialOpts, PeerCondition},
    dummy, ConnectionDenied, ConnectionHandler, ConnectionId, DialFailure, FromSwarm,
    NetworkBehaviour, ToSwarm,
};
use rand_core::{OsRng, RngCore};

use crate::v2::server::handler::{
    dial_back,
    dial_request::{self, DialBackCommand, DialBackStatus},
    Handler,
};

pub struct Behaviour<R = OsRng>
where
    R: Clone + Send + RngCore + 'static,
{
    dialing_dial_back: HashMap<ConnectionId, DialBackCommand>,
    pending_events: VecDeque<
        ToSwarm<
            <Self as NetworkBehaviour>::ToSwarm,
            <<Self as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::FromBehaviour,
        >,
    >,
    rng: R,
}

impl Default for Behaviour<OsRng> {
    fn default() -> Self {
        Self::new(OsRng)
    }
}

impl<R> Behaviour<R>
where
    R: RngCore + Send + Clone + 'static,
{
    pub fn new(rng: R) -> Self {
        Self {
            dialing_dial_back: HashMap::new(),
            pending_events: VecDeque::new(),
            rng,
        }
    }
}

impl<R> NetworkBehaviour for Behaviour<R>
where
    R: RngCore + Send + Clone + 'static,
{
    type ConnectionHandler = Handler<R>;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        _local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        Ok(Either::Right(dial_request::Handler::new(
            peer,
            remote_addr.clone(),
            self.rng.clone(),
        )))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        Ok(match self.dialing_dial_back.remove(&connection_id) {
            Some(cmd) => Either::Left(Either::Left(dial_back::Handler::new(cmd))),
            None => Either::Left(Either::Right(dummy::ConnectionHandler)),
        })
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        if let FromSwarm::DialFailure(DialFailure { connection_id, .. }) = event {
            if let Some(DialBackCommand { back_channel, .. }) =
                self.dialing_dial_back.remove(&connection_id)
            {
                let dial_back_status = DialBackStatus::DialErr;
                let _ = back_channel.send(Err(dial_back_status));
            }
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        event: <Handler<R> as ConnectionHandler>::ToBehaviour,
    ) {
        match event {
            Either::Left(Either::Left(Ok(_))) => {}
            Either::Left(Either::Left(Err(e))) => {
                tracing::debug!("dial back error: {e:?}");
            }
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            Either::Left(Either::Right(v)) => libp2p_core::util::unreachable(v),
            Either::Right(Either::Left(cmd)) => {
                let addr = cmd.addr.clone();
                let opts = DialOpts::peer_id(peer_id)
                    .addresses(Vec::from([addr]))
                    .condition(PeerCondition::Always)
                    .allocate_new_port()
                    .build();
                let conn_id = opts.connection_id();
                self.dialing_dial_back.insert(conn_id, cmd);
                self.pending_events.push_back(ToSwarm::Dial { opts });
            }
            Either::Right(Either::Right(status_update)) => self
                .pending_events
                .push_back(ToSwarm::GenerateEvent(status_update)),
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, <Handler<R> as ConnectionHandler>::FromBehaviour>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        Poll::Pending
    }
}

#[derive(Debug)]
pub struct Event {
    /// All address that were submitted for testing.
    pub all_addrs: Vec<Multiaddr>,
    /// The address that was eventually tested.
    pub tested_addr: Multiaddr,
    /// The peer id of the client that submitted addresses for testing.
    pub client: PeerId,
    /// The amount of data that was requested by the server and was transmitted.
    pub data_amount: usize,
    /// The result of the test.
    pub result: Result<(), io::Error>,
}
