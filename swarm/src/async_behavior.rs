use std::{collections::HashMap, hash::Hash, ops::{Deref, DerefMut}, task::{Context, Poll}};

use futures::{channel::{mpsc, oneshot}, SinkExt, StreamExt};

use crate::{NetworkBehaviour, ToSwarm};

/// Trait for mapping commands to behavior methods, and behavior
/// events to query results.
pub trait QueryMapper: NetworkBehaviour + Sized {
    /// Id for matching swarm events to a user-invoked query.
    type QueryId: Hash + Eq;
    /// Commands that can be send from [`Control`] and mapped
    /// to the behavior's functions.
    type Command: Send;
    /// Result of a query.
    type Result: Send;

    /// Handle a command from the control.
    /// 
    /// The implementation should call the matching method on the inner
    /// behavior and return the identifier that is used in resulting
    /// swarm events.
    fn handle_command(
        &mut self, 
        command: Self::Command
    ) -> Result<Self::QueryId, Self::Result>;

    /// Extract the query ID and the progress step from a swarm event.
    fn extract_id(event: &Self::ToSwarm) -> Option<(Self::QueryId, Step)>;
    
    /// Map the network behavior event to a query result.
    /// 
    /// Should return the original event if the event type doesn't match any
    /// expected type.
    fn map_event(event: Self::ToSwarm) -> Result<Self::Result, Self::ToSwarm>;
}

/// Control handle for sending commands to the network behavior and
/// receiving the result as future or stream.
pub struct Control<T: QueryMapper> {
    // Command hannel to the network behavior.
    sender: mpsc::Sender<(T::Command, ResultChannel<T::Result>)>,
}

impl<T: QueryMapper> Control<T> {
    /// Execute a command with a single result on the behavior.
    /// 
    /// This will send the command through a channel to the behavior.
    /// The behavior will return the result in an async manner once the query or request
    /// resolved in the swarm.
    pub async fn execute(&mut self, cmd: T::Command) -> Result<T::Result, Disconnected> {
        let (tx, rx) = oneshot::channel();
       self.sender.send((cmd, ResultChannel::Oneshot(tx))).await.map_err(|_|Disconnected)?;
        rx.await.map_err(|_|Disconnected)
    }

    /// Execute a command with a stream of results  on the behavior.
    /// 
    /// This will send the command through a channel to the behavior.
    /// The behavior will forward all related swarm events for the command's query through 
    /// the returned mpsc channel.
    pub async fn execute_with_result_stream(&mut self, cmd: T::Command, cap: usize) -> Result<mpsc::Receiver<T::Result>, Disconnected> {
        let (tx, rx) = mpsc::channel(cap);
        self.sender.send((cmd, ResultChannel::Mpsc(tx))).await.map_err(|_|Disconnected)?;
        Ok(rx)
    }
}

/// Asynchronous wrapper for a [`NetworkBehavior`].
/// 
/// The wrapper will receive commands from the [`Control`], track
/// pending queries, and return the results for pending queries directly
/// to the caller by intercepting all events from the inner behavior.
pub struct Async<T: QueryMapper> {
    inner: T,
    command_rx: mpsc::Receiver<(T::Command, ResultChannel<T::Result>)>,
    pending_queries: HashMap<T::QueryId, ResultChannel<T::Result>>,
    pending_result: Option<(T::QueryId, T::Result, Step)>
}

impl<T: QueryMapper> Async<T> {
    pub fn new(inner: T, cap: usize) -> (Self, Control<T>) {
        let (tx, rx) = mpsc::channel(cap);
        let ctrl =  Control {
            sender: tx
        };
        (Self {
            inner,
            command_rx: rx,
            pending_queries: HashMap::new(),
            pending_result: None
        }, ctrl)
    }
}

impl<T: QueryMapper> NetworkBehaviour for Async<T> {
    type ConnectionHandler = T::ConnectionHandler;
    type ToSwarm = T::ToSwarm;

    fn handle_pending_inbound_connection(
            &mut self,
            connection_id: crate::ConnectionId,
            local_addr: &libp2p_core::Multiaddr,
            remote_addr: &libp2p_core::Multiaddr,
        ) -> Result<(), crate::ConnectionDenied> {
        self.inner.handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_pending_outbound_connection(
            &mut self,
            connection_id: crate::ConnectionId,
            maybe_peer: Option<libp2p_core::PeerId>,
            addresses: &[libp2p_core::Multiaddr],
            effective_role: libp2p_core::Endpoint,
        ) -> Result<Vec<libp2p_core::Multiaddr>, crate::ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(connection_id, maybe_peer, addresses, effective_role)
    }

    fn handle_established_inbound_connection(
            &mut self,
            connection_id: crate::ConnectionId,
            peer: libp2p_core::PeerId,
            local_addr: &libp2p_core::Multiaddr,
            remote_addr: &libp2p_core::Multiaddr,
        ) -> Result<crate::THandler<Self>, crate::ConnectionDenied> {
        self.inner.handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)
    }

    fn handle_established_outbound_connection(
            &mut self,
            connection_id: crate::ConnectionId,
            peer: libp2p_core::PeerId,
            addr: &libp2p_core::Multiaddr,
            role_override: libp2p_core::Endpoint,
            port_use: libp2p_core::transport::PortUse,
        ) -> Result<crate::THandler<Self>, crate::ConnectionDenied> {
        self.inner.handle_established_outbound_connection(connection_id, peer, addr, role_override, port_use)
    }

    fn on_connection_handler_event(
            &mut self,
            peer_id: libp2p_core::PeerId,
            connection_id: crate::ConnectionId,
            event: crate::THandlerOutEvent<Self>,
        ) {
        self.inner.on_connection_handler_event(peer_id, connection_id, event);
    }

    fn on_swarm_event(&mut self, event: crate::FromSwarm) {
        self.inner.on_swarm_event(event)
    }

    fn poll(&mut self, cx: &mut std::task::Context<'_>)
            -> Poll<crate::ToSwarm<Self::ToSwarm, crate::THandlerInEvent<Self>>> {
        loop {
            if let Some((id, result, step)) = self.pending_result.take() {
                let channel = self.pending_queries.get_mut(&id).expect("Query is pending");
                if !channel.poll_ready(cx, step) {
                    self.pending_result = Some((id, result, step));
                    return Poll::Pending;
                }
                match step {
                    Step::Intermediate => channel.send(result),
                    Step::Last => {
                        let channel = self.pending_queries.remove(&id).expect("Id should be in queries.");
                        channel.send_final(result)
                    } 
                } 
            }

            // Poll for new commands from the control interface.
            if let Poll::Ready(Some((cmd, tx))) = self.command_rx.poll_next_unpin(cx) {
                match self.inner.handle_command(cmd) {
                    Ok(id) => {
                        // Store channel for the query results.
                        let _ = self.pending_queries.insert(id, tx);
                    }
                    Err(res) => tx.send_final(res),
                }
                continue;
            }

            // Intercept events from the inner behavior and return the results for
            // pending queries through the stored channel.
            match self.inner.poll(cx) {
                Poll::Ready(ToSwarm::GenerateEvent(event)) => {
                    let Some((id, step)) = T::extract_id(&event).filter(|(id, _) | self.pending_queries.contains_key(id)) 
                    else {
                        return Poll::Ready(ToSwarm::GenerateEvent(event))
                    };
                    match T::map_event(event) {
                        Ok(res) => self.pending_result = Some((id, res, step)),
                        Err(event) => return Poll::Ready(ToSwarm::GenerateEvent(event)),
                    }
                }
                poll => return poll
            }
        }

    }
}

impl<T:QueryMapper> Deref for Async<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T:QueryMapper> DerefMut for Async<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Channel for returning the result(s) for a query.
pub enum ResultChannel<T> {
    /// Single result.
    Oneshot(oneshot::Sender<T>),
    /// Stream of results.
    Mpsc(mpsc::Sender<T>)
}

impl<T> ResultChannel<T> {
    /// Poll if the channel is ready to received a next event
    pub fn poll_ready(&mut self, cx: &mut Context, step: Step) -> bool {
        if !matches!((&self, step), (ResultChannel::Oneshot(_), Step::Intermediate)) {
            return false;
        }
        match self {
            ResultChannel::Oneshot(tx) => !tx.is_canceled(),
            ResultChannel::Mpsc(tx) => matches!(tx.poll_ready(cx), Poll::Ready(Ok(())))
        }
    }

    /// Send the final result for a pending query.
    pub fn send_final(self, t: T) {
        match self {
            ResultChannel::Oneshot(tx) => {
                let _ = tx.send(t);
            }
            ResultChannel::Mpsc(mut tx) => {
                let _ = tx.start_send(t);
            }
        }
    }
    
    /// Send an intermediate result for a pending query.
    pub fn send(&mut self, t: T) {
        if let ResultChannel::Mpsc(tx) = self {
            let _ = tx.start_send(t);
        }
    }
}

/// Result
pub struct QueryResult<T> {
    pub id: T,
    pub is_final: bool
}

/// The behavior, and thus also the swarm, disconnected.
#[derive(Debug)]
pub struct Disconnected;

#[derive(Debug, Clone, Copy)]
pub enum Step {
    Intermediate,
    Last,
}