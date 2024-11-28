use std::{
    collections::{HashSet, VecDeque},
    convert::Infallible,
    task::{Context, Poll},
};

use libp2p_core::{
    multiaddr::Protocol,
    transport::{ListenerId, PortUse},
    Endpoint, Multiaddr,
};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    derive_prelude::NewListener, dummy, ConnectionDenied, ConnectionId, FromSwarm, ListenOpts,
    ListenerClosed, ListenerError, NetworkBehaviour, NewListenAddr, Swarm, SwarmEvent, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p_swarm_test::SwarmExt;

#[async_std::test]
async fn behaviour_listener() {
    let mut swarm = Swarm::new_ephemeral(|_| Behaviour::default());
    let addr: Multiaddr = Protocol::Memory(0).into();
    let id = swarm.behaviour_mut().listen(addr.clone());

    let address = swarm
        .wait(|e| match e {
            SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } => {
                assert_eq!(listener_id, id);
                Some(address)
            }
            _ => None,
        })
        .await;

    swarm.behaviour_mut().stop_listening(id);

    swarm
        .wait(|e| match e {
            SwarmEvent::ListenerClosed {
                listener_id,
                addresses,
                reason,
            } => {
                assert_eq!(listener_id, id);
                assert!(addresses.contains(&address));
                assert!(reason.is_ok());
                Some(())
            }
            _ => None,
        })
        .await;
}

#[derive(Default)]
struct Behaviour {
    events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,
    listeners: HashSet<ListenerId>,
}

impl Behaviour {
    pub(crate) fn listen(&mut self, addr: Multiaddr) -> ListenerId {
        let opts = ListenOpts::new(addr);
        let listener_id = opts.listener_id();
        assert!(!self.listeners.contains(&listener_id));
        self.events.push_back(ToSwarm::ListenOn { opts });
        self.listeners.insert(listener_id);

        listener_id
    }

    pub(crate) fn stop_listening(&mut self, id: ListenerId) {
        self.events.push_back(ToSwarm::RemoveListener { id });
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        _: THandlerOutEvent<Self>,
    ) {
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::NewListener(NewListener { listener_id }) => {
                assert!(self.listeners.contains(&listener_id));
            }
            FromSwarm::NewListenAddr(NewListenAddr { listener_id, .. }) => {
                assert!(self.listeners.contains(&listener_id));
            }
            FromSwarm::ListenerError(ListenerError { listener_id, err }) => {
                panic!("Error for listener {listener_id:?}: {err}");
            }
            FromSwarm::ListenerClosed(ListenerClosed {
                listener_id,
                reason,
            }) => {
                assert!(self.listeners.contains(&listener_id));
                assert!(reason.is_ok());
                self.listeners.remove(&listener_id);
                assert!(!self.listeners.contains(&listener_id));
            }
            _ => {}
        }
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
