// Copyright 2020 Parity Technologies (UK) Ltd.
// Copyright 2021 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use crate::v2::client::RelayedConnection;
use crate::v2::RequestId;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future::{ready, BoxFuture, FutureExt, Ready};
use futures::ready;
use futures::sink::SinkExt;
use futures::stream::SelectAll;
use futures::stream::{Stream, StreamExt};
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::transport::{ListenerId, TransportError, TransportEvent};
use libp2p_core::{PeerId, Transport};
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;

/// A [`Transport`] enabling client relay capabilities.
///
/// Note: The transport only handles listening and dialing on relayed [`Multiaddr`], and depends on
/// an other transport to do the actual transmission of data. They should be combined through the
/// [`OrTransport`](libp2p_core::transport::choice::OrTransport).
///
/// Allows the local node to:
///
/// 1. Establish relayed connections by dialing `/p2p-circuit` addresses.
///
///    ```
///    # use libp2p_core::{Multiaddr, multiaddr::{Protocol}, Transport, PeerId};
///    # use libp2p_core::transport::memory::MemoryTransport;
///    # use libp2p_core::transport::choice::OrTransport;
///    # use libp2p_relay::v2::client;
///    let actual_transport = MemoryTransport::default();
///    let (relay_transport, behaviour) = client::Client::new_transport_and_behaviour(
///        PeerId::random(),
///    );
///    let mut transport = OrTransport::new(relay_transport, actual_transport);
///    # let relay_id = PeerId::random();
///    # let destination_id = PeerId::random();
///    let dst_addr_via_relay = Multiaddr::empty()
///        .with(Protocol::Memory(40)) // Relay address.
///        .with(Protocol::P2p(relay_id.into())) // Relay peer id.
///        .with(Protocol::P2pCircuit) // Signal to connect via relay and not directly.
///        .with(Protocol::P2p(destination_id.into())); // Destination peer id.
///    transport.dial(dst_addr_via_relay).unwrap();
///    ```
///
/// 3. Listen for incoming relayed connections via specific relay.
///
///    ```
///    # use libp2p_core::{Multiaddr, multiaddr::{Protocol}, Transport, PeerId};
///    # use libp2p_core::transport::memory::MemoryTransport;
///    # use libp2p_core::transport::choice::OrTransport;
///    # use libp2p_relay::v2::client;
///    # let relay_id = PeerId::random();
///    # let local_peer_id = PeerId::random();
///    let actual_transport = MemoryTransport::default();
///    let (relay_transport, behaviour) = client::Client::new_transport_and_behaviour(
///       local_peer_id,
///    );
///    let mut transport = OrTransport::new(relay_transport, actual_transport);
///    let relay_addr = Multiaddr::empty()
///        .with(Protocol::Memory(40)) // Relay address.
///        .with(Protocol::P2p(relay_id.into())) // Relay peer id.
///        .with(Protocol::P2pCircuit); // Signal to listen via remote relay node.
///    transport.listen_on(relay_addr).unwrap();
///    ```
pub struct ClientTransport {
    to_behaviour: mpsc::Sender<TransportToBehaviourMsg>,
    pending_to_behaviour: VecDeque<TransportToBehaviourMsg>,
    listeners: SelectAll<RelayListener>,
}

impl ClientTransport {
    /// Create a new [`ClientTransport`].
    ///
    /// Note: The transport only handles listening and dialing on relayed [`Multiaddr`], and depends on
    /// an other transport to do the actual transmission of data. They should be combined through the
    /// [`OrTransport`](libp2p_core::transport::choice::OrTransport).
    ///
    /// ```
    /// # use libp2p_core::{Multiaddr, multiaddr::{Protocol}, Transport, PeerId};
    /// # use libp2p_core::transport::memory::MemoryTransport;
    /// # use libp2p_core::transport::choice::OrTransport;
    /// # use libp2p_relay::v2::client;
    /// let actual_transport = MemoryTransport::default();
    /// let (relay_transport, behaviour) = client::Client::new_transport_and_behaviour(
    ///     PeerId::random(),
    /// );
    ///
    /// // To reduce unnecessary connection attempts, put `relay_transport` first.
    /// let mut transport = OrTransport::new(relay_transport, actual_transport);
    /// ```
    pub(crate) fn new() -> (Self, mpsc::Receiver<TransportToBehaviourMsg>) {
        let (to_behaviour, from_transport) = mpsc::channel(0);
        let transport = ClientTransport {
            to_behaviour,
            pending_to_behaviour: VecDeque::new(),
            listeners: SelectAll::new(),
        };
        (transport, from_transport)
    }
}

impl Transport for ClientTransport {
    type Output = RelayedConnection;
    type Error = RelayError;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = RelayedDial;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let (relay_peer_id, relay_addr) = match parse_relayed_multiaddr(addr)? {
            RelayedMultiaddr {
                relay_peer_id: None,
                relay_addr: _,
                ..
            } => return Err(RelayError::MissingDstPeerId.into()),
            RelayedMultiaddr {
                relay_peer_id: _,
                relay_addr: None,
                ..
            } => return Err(RelayError::MissingRelayAddr.into()),
            RelayedMultiaddr {
                relay_peer_id: Some(peer_id),
                relay_addr: Some(addr),
                ..
            } => (peer_id, addr),
        };

        let (to_listener, from_behaviour) = mpsc::channel(0);
        self.pending_to_behaviour
            .push_back(TransportToBehaviourMsg::ListenReq {
                relay_peer_id,
                relay_addr,
                to_listener,
            });

        let listener_id = ListenerId::new();
        let listener = RelayListener {
            listener_id,
            queued_events: Default::default(),
            from_behaviour,
            is_closed: false,
        };
        self.listeners.push(listener);
        Ok(listener_id)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(listener) = self.listeners.iter_mut().find(|l| l.listener_id == id) {
            listener.close(Ok(()));
            true
        } else {
            false
        }
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let RelayedMultiaddr {
            relay_peer_id,
            relay_addr,
            dst_peer_id,
            dst_addr,
        } = parse_relayed_multiaddr(addr)?;

        // TODO: In the future we might want to support dialing a relay by its address only.
        let relay_peer_id = relay_peer_id.ok_or(RelayError::MissingRelayPeerId)?;
        let relay_addr = relay_addr.ok_or(RelayError::MissingRelayAddr)?;
        let dst_peer_id = dst_peer_id.ok_or(RelayError::MissingDstPeerId)?;

        let mut to_behaviour = self.to_behaviour.clone();
        Ok(async move {
            let (tx, rx) = oneshot::channel();
            to_behaviour
                .send(TransportToBehaviourMsg::DialReq {
                    request_id: RequestId::new(),
                    relay_addr,
                    relay_peer_id,
                    dst_addr,
                    dst_peer_id,
                    send_back: tx,
                })
                .await?;
            let stream = rx.await?.map_err(|()| RelayError::Connect)?;
            Ok(stream)
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>>
    where
        Self: Sized,
    {
        // [`Transport::dial_as_listener`] is used for NAT and firewall
        // traversal. One would coordinate such traversal via a previously
        // established relayed connection, but never using a relayed connection
        // itself.
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn address_translation(&self, _server: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>>
    where
        Self: Sized,
    {
        loop {
            if !self.pending_to_behaviour.is_empty() {
                match self.to_behaviour.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        let msg = self
                            .pending_to_behaviour
                            .pop_front()
                            .expect("Called !is_empty().");
                        let _ = self.to_behaviour.start_send(msg);
                        continue;
                    }
                    Poll::Ready(Err(_)) => unreachable!("Receiver is never dropped."),
                    Poll::Pending => {}
                }
            }
            match self.listeners.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => return Poll::Ready(event),
                _ => return Poll::Pending,
            }
        }
    }
}

#[derive(Default)]
struct RelayedMultiaddr {
    relay_peer_id: Option<PeerId>,
    relay_addr: Option<Multiaddr>,
    dst_peer_id: Option<PeerId>,
    dst_addr: Option<Multiaddr>,
}

/// Parse a [`Multiaddr`] containing a [`Protocol::P2pCircuit`].
fn parse_relayed_multiaddr(
    addr: Multiaddr,
) -> Result<RelayedMultiaddr, TransportError<RelayError>> {
    if !addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
        return Err(TransportError::MultiaddrNotSupported(addr));
    }

    let mut relayed_multiaddr = RelayedMultiaddr::default();

    let mut before_circuit = true;
    for protocol in addr.into_iter() {
        match protocol {
            Protocol::P2pCircuit => {
                if before_circuit {
                    before_circuit = false;
                } else {
                    return Err(RelayError::MultipleCircuitRelayProtocolsUnsupported.into());
                }
            }
            Protocol::P2p(hash) => {
                let peer_id = PeerId::from_multihash(hash).map_err(|_| RelayError::InvalidHash)?;

                if before_circuit {
                    if relayed_multiaddr.relay_peer_id.is_some() {
                        return Err(RelayError::MalformedMultiaddr.into());
                    }
                    relayed_multiaddr.relay_peer_id = Some(peer_id)
                } else {
                    if relayed_multiaddr.dst_peer_id.is_some() {
                        return Err(RelayError::MalformedMultiaddr.into());
                    }
                    relayed_multiaddr.dst_peer_id = Some(peer_id)
                }
            }
            p => {
                if before_circuit {
                    relayed_multiaddr
                        .relay_addr
                        .get_or_insert(Multiaddr::empty())
                        .push(p);
                } else {
                    relayed_multiaddr
                        .dst_addr
                        .get_or_insert(Multiaddr::empty())
                        .push(p);
                }
            }
        }
    }

    Ok(relayed_multiaddr)
}

pub struct RelayListener {
    listener_id: ListenerId,
    /// Queue of events to report when polled.
    queued_events: VecDeque<<Self as Stream>::Item>,
    /// Channel for messages from the behaviour [`Handler`][super::handler::Handler].
    from_behaviour: mpsc::Receiver<ToListenerMsg>,
    /// The listener can be closed either manually with [`Transport::remove_listener`] or if
    /// the sender side of the `from_behaviour` channel is dropped.
    is_closed: bool,
}

impl RelayListener {
    /// Close the listener.
    ///
    /// This will create a [`TransportEvent::ListenerClosed`] event
    /// and terminate the stream once all remaining events in queue have
    /// been reported.
    fn close(&mut self, reason: Result<(), RelayError>) {
        self.queued_events
            .push_back(TransportEvent::ListenerClosed {
                listener_id: self.listener_id,
                reason,
            });
        self.is_closed = true;
    }
}

impl Stream for RelayListener {
    type Item = TransportEvent<<ClientTransport as Transport>::ListenerUpgrade, RelayError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.queued_events.pop_front() {
                return Poll::Ready(Some(event));
            }

            if self.is_closed {
                // Terminate the stream if the listener closed and all remaining events have been reported.
                return Poll::Ready(None);
            }

            let msg = match ready!(self.from_behaviour.poll_next_unpin(cx)) {
                Some(msg) => msg,
                None => {
                    // Sender of `from_behaviour` has been dropped, signaling listener to close.
                    self.close(Ok(()));
                    continue;
                }
            };

            match msg {
                ToListenerMsg::Reservation(Ok(Reservation { addrs })) => {
                    debug_assert!(
                        self.queued_events.is_empty(),
                        "Assert empty due to previous `pop_front` attempt."
                    );
                    // Returned as [`ListenerEvent::NewAddress`] in next iteration of loop.
                    self.queued_events = addrs
                        .into_iter()
                        .map(|listen_addr| TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr,
                        })
                        .collect();
                }
                ToListenerMsg::IncomingRelayedConnection {
                    stream,
                    src_peer_id,
                    relay_addr,
                    relay_peer_id: _,
                } => {
                    let listener_id = self.listener_id;

                    self.queued_events.push_back(TransportEvent::Incoming {
                        upgrade: ready(Ok(stream)),
                        listener_id,
                        local_addr: relay_addr.with(Protocol::P2pCircuit),
                        send_back_addr: Protocol::P2p(src_peer_id.into()).into(),
                    })
                }
                ToListenerMsg::Reservation(Err(())) => self.close(Err(RelayError::Reservation)),
            };
        }
    }
}

pub type RelayedDial = BoxFuture<'static, Result<RelayedConnection, RelayError>>;

/// Error that occurred during relay connection setup.
#[derive(Debug, Error)]
pub enum RelayError {
    #[error("Missing relay peer id.")]
    MissingRelayPeerId,
    #[error("Missing relay address.")]
    MissingRelayAddr,
    #[error("Missing destination peer id.")]
    MissingDstPeerId,
    #[error("Invalid peer id hash.")]
    InvalidHash,
    #[error("Failed to send message to relay behaviour: {0:?}")]
    SendingMessageToBehaviour(#[from] mpsc::SendError),
    #[error("Response from behaviour was canceled")]
    ResponseFromBehaviourCanceled(#[from] oneshot::Canceled),
    #[error(
        "Address contains multiple circuit relay protocols (`p2p-circuit`) which is not supported."
    )]
    MultipleCircuitRelayProtocolsUnsupported,
    #[error("One of the provided multiaddresses is malformed.")]
    MalformedMultiaddr,
    #[error("Failed to get Reservation.")]
    Reservation,
    #[error("Failed to connect to destination.")]
    Connect,
}

impl From<RelayError> for TransportError<RelayError> {
    fn from(error: RelayError) -> Self {
        TransportError::Other(error)
    }
}

/// Message from the [`ClientTransport`] to the [`Relay`](crate::v2::relay::Relay)
/// [`NetworkBehaviour`](libp2p_swarm::NetworkBehaviour).
pub enum TransportToBehaviourMsg {
    /// Dial destination node via relay node.
    DialReq {
        request_id: RequestId,
        relay_addr: Multiaddr,
        relay_peer_id: PeerId,
        dst_addr: Option<Multiaddr>,
        dst_peer_id: PeerId,
        send_back: oneshot::Sender<Result<RelayedConnection, ()>>,
    },
    /// Listen for incoming relayed connections via relay node.
    ListenReq {
        relay_peer_id: PeerId,
        relay_addr: Multiaddr,
        to_listener: mpsc::Sender<ToListenerMsg>,
    },
}

#[allow(clippy::large_enum_variant)]
pub enum ToListenerMsg {
    Reservation(Result<Reservation, ()>),
    IncomingRelayedConnection {
        stream: RelayedConnection,
        src_peer_id: PeerId,
        relay_peer_id: PeerId,
        relay_addr: Multiaddr,
    },
}

pub struct Reservation {
    pub(crate) addrs: Vec<Multiaddr>,
}
