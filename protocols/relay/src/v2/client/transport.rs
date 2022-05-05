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
use futures::future::{ready, BoxFuture, Future, FutureExt, Ready};
use futures::ready;
use futures::sink::SinkExt;
use futures::stream::{Stream, StreamExt};
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::transport::{ListenerEvent, TransportError};
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
#[derive(Clone)]
pub struct ClientTransport {
    to_behaviour: mpsc::Sender<TransportToBehaviourMsg>,
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

        (ClientTransport { to_behaviour }, from_transport)
    }
}

impl Transport for ClientTransport {
    type Output = RelayedConnection;
    type Error = RelayError;
    type Listener = RelayListener;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = RelayedDial;

    fn listen_on(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Listener, TransportError<Self::Error>> {
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
        let mut to_behaviour = self.to_behaviour.clone();
        let msg_to_behaviour = Some(
            async move {
                to_behaviour
                    .send(TransportToBehaviourMsg::ListenReq {
                        relay_peer_id,
                        relay_addr,
                        to_listener,
                    })
                    .await
            }
            .boxed(),
        );

        Ok(RelayListener {
            queued_new_addresses: Default::default(),
            from_behaviour,
            msg_to_behaviour,
        })
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
    queued_new_addresses: VecDeque<Multiaddr>,
    from_behaviour: mpsc::Receiver<ToListenerMsg>,
    msg_to_behaviour: Option<BoxFuture<'static, Result<(), mpsc::SendError>>>,
}

impl Unpin for RelayListener {}

impl Stream for RelayListener {
    type Item =
        Result<ListenerEvent<Ready<Result<RelayedConnection, RelayError>>, RelayError>, RelayError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(msg) = &mut self.msg_to_behaviour {
                match Future::poll(msg.as_mut(), cx) {
                    Poll::Ready(Ok(())) => self.msg_to_behaviour = None,
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e.into()))),
                    Poll::Pending => {}
                }
            }

            if let Some(addr) = self.queued_new_addresses.pop_front() {
                return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr))));
            }

            let msg = match ready!(self.from_behaviour.poll_next_unpin(cx)) {
                Some(msg) => msg,
                None => {
                    // Sender of `from_behaviour` has been dropped, signaling listener to close.
                    return Poll::Ready(None);
                }
            };

            let result = match msg {
                ToListenerMsg::Reservation(Ok(Reservation { addrs })) => {
                    debug_assert!(
                        self.queued_new_addresses.is_empty(),
                        "Assert empty due to previous `pop_front` attempt."
                    );
                    // Returned as [`ListenerEvent::NewAddress`] in next iteration of loop.
                    self.queued_new_addresses = addrs.into();

                    continue;
                }
                ToListenerMsg::IncomingRelayedConnection {
                    stream,
                    src_peer_id,
                    relay_addr,
                    relay_peer_id: _,
                } => Ok(ListenerEvent::Upgrade {
                    upgrade: ready(Ok(stream)),
                    local_addr: relay_addr.with(Protocol::P2pCircuit),
                    remote_addr: Protocol::P2p(src_peer_id.into()).into(),
                }),
                ToListenerMsg::Reservation(Err(())) => Err(RelayError::Reservation),
            };

            return Poll::Ready(Some(result));
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
