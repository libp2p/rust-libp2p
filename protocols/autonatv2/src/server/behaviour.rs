use std::{
    collections::{HashMap, VecDeque},
    task::{Context, Poll},
};

use crate::server::handler::dial_request::DialBack;
use either::Either;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dial_opts::DialOpts, ConnectionDenied, ConnectionHandler, ConnectionId, DialError, DialFailure,
    FromSwarm, NetworkBehaviour, NotifyHandler, ToSwarm,
};
use rand_core::{OsRng, RngCore};

use super::handler::{
    dial_back,
    dial_request::{self, DialBackCommand},
    Handler,
};

pub struct Behaviour<R = OsRng>
where
    R: Clone + Send + RngCore + 'static,
{
    handlers: HashMap<(Multiaddr, PeerId), ConnectionId>,
    dialing_dial_back: HashMap<(Multiaddr, PeerId), VecDeque<DialBackCommand>>,
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
            handlers: HashMap::new(),
            dialing_dial_back: HashMap::new(),
            pending_events: VecDeque::new(),
            rng,
        }
    }

    fn poll_pending_events(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<
        ToSwarm<
            <Self as NetworkBehaviour>::ToSwarm,
            <<Self as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::FromBehaviour,
        >,
    > {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        Poll::Pending
    }
}

impl<R> NetworkBehaviour for Behaviour<R>
where
    R: RngCore + Send + Clone + 'static,
{
    type ConnectionHandler = Handler<R>;

    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        Ok(Either::Right(dial_request::Handler::new(
            remote_addr.clone(),
            self.rng.clone(),
        )))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        _role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<<Self as NetworkBehaviour>::ConnectionHandler, ConnectionDenied> {
        if port_use == PortUse::New {
            self.handlers.insert(
                (
                    addr.iter()
                        .filter(|e| !matches!(e, Protocol::P2p(_)))
                        .collect(),
                    peer,
                ),
                connection_id,
            );
            println!("Found handler");
        }
        Ok(Either::Left(dial_back::Handler::new()))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        println!("EVENT: {event:?}");
        match event {
            FromSwarm::DialFailure(DialFailure {
                error: DialError::Transport(pairs),
                ..
            }) => {
                println!("Failed to dial");
                for (addr, _) in pairs.iter() {
                    if let Some((_, p)) = self.dialing_dial_back.keys().find(|(a, _)| a == addr) {
                        let cmds = self.dialing_dial_back.remove(&(addr.clone(), *p)).unwrap();
                        for cmd in cmds {
                            let _ = cmd.back_channel.send(DialBack::Dial);
                        }
                    }
                }
            }
            FromSwarm::DialFailure(m) => {
                println!("{m:?}");
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        event: <Handler<R> as ConnectionHandler>::ToBehaviour,
    ) {
        if let Either::Right(m) = event {
            match m {
                Ok(cmd) => {
                    let addr = cmd.addr.clone();
                    if let Some(connection_id) = self.handlers.get(&(addr.clone(), peer_id)) {
                        self.pending_events.push_back(ToSwarm::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::One(*connection_id),
                            event: Either::Left(cmd),
                        });
                    } else {
                        if let Some(pending) =
                            self.dialing_dial_back.get_mut(&(addr.clone(), peer_id))
                        {
                            pending.push_back(cmd);
                        } else {
                            self.pending_events.push_back(ToSwarm::Dial {
                                /*                                opts: DialOpts::peer_id(peer_id)
                                                                    .addresses(Vec::from([addr.clone()]))
                                                                    .condition(PeerCondition::Always)
                                                                    .allocate_new_port()
                                                                    .build(),
                                */
                                opts: DialOpts::unknown_peer_id()
                                    .address(addr.clone())
                                    .allocate_new_port()
                                    .build(),
                            });
                            self.dialing_dial_back
                                .insert((addr, peer_id), VecDeque::from([cmd]));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("incoming dial request failed: {}", e);
                }
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, <Handler<R> as ConnectionHandler>::FromBehaviour>> {
        let pending_event = self.poll_pending_events(cx);
        if pending_event.is_ready() {
            return pending_event;
        }
        if let Some((addr, peer)) = self
            .dialing_dial_back
            .keys()
            .filter(|k| self.handlers.contains_key(*k))
            .next()
            .cloned()
        {
            let cmds = self
                .dialing_dial_back
                .remove(&(addr.clone(), peer))
                .unwrap();
            let cmd_n = cmds.len();
            for cmd in cmds {
                self.pending_events.push_back(ToSwarm::NotifyHandler {
                    peer_id: peer.clone(),
                    handler: NotifyHandler::One(self.handlers[&(addr.clone(), peer)]),
                    event: Either::Left(cmd),
                });
            }
            if cmd_n > 0 {
                return self.poll_pending_events(cx);
            }
        }
        Poll::Pending
    }
}
