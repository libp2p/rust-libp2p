use crate::behaviour::BehaviourToTransportMsg;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Future, FutureExt};
use futures::sink::SinkExt;
use futures::stream::{Stream, StreamExt};
use libp2p_core::either::{EitherError, EitherFuture, EitherOutput};
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::transport::{ListenerEvent, TransportError};
use libp2p_core::{PeerId, Transport};
use libp2p_swarm::NegotiatedSubstream;
use pin_project::pin_project;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

pub enum TransportToBehaviourMsg {
    DialRequest {
        relay_addr: Multiaddr,
        relay_peer_id: PeerId,
        destination_addr: Multiaddr,
        destination_peer_id: PeerId,
        send_back: oneshot::Sender<NegotiatedSubstream>,
    },
    ListenRequest {
        address: Multiaddr,
        peer_id: PeerId,
    },
}

#[derive(Clone)]
pub struct RelayTransportWrapper<T: Clone> {
    to_behaviour: mpsc::Sender<TransportToBehaviourMsg>,
    // TODO: Can we get around the arc mutex?
    from_behaviour: Arc<Mutex<mpsc::Receiver<BehaviourToTransportMsg>>>,

    inner_transport: T,
}

impl<T: Clone> RelayTransportWrapper<T> {
    /// Wrap an existing [`Transport`] into a [`RelayTransportWrapper`] allowing dialing and
    /// listening for both relayed as well as native connections.
    ///
    ///```
    /// # use libp2p_core::transport::dummy::DummyTransport;
    /// # use libp2p_relay::Relay;
    /// # use libp2p_relay::transport::RelayTransportWrapper;
    ///
    /// let inner_transport = DummyTransport::<()>::new();
    /// let (wrapper_transport, (channels)) = RelayTransportWrapper::new(inner_transport);
    /// let (to_transport, from_transport) = channels;
    ///
    /// let behaviour = Relay::new(to_transport, from_transport);
    ///```
    pub fn new(
        t: T,
    ) -> (
        Self,
        (
            mpsc::Sender<BehaviourToTransportMsg>,
            mpsc::Receiver<TransportToBehaviourMsg>,
        ),
    ) {
        let (to_behaviour, from_transport) = mpsc::channel(0);
        let (to_transport, from_behaviour) = mpsc::channel(0);

        let transport = RelayTransportWrapper {
            to_behaviour,
            from_behaviour: Arc::new(Mutex::new(from_behaviour)),

            inner_transport: t,
        };

        (transport, (to_transport, from_transport))
    }
}

impl<T: Transport + Clone> Transport for RelayTransportWrapper<T> {
    type Output = EitherOutput<<T as Transport>::Output, NegotiatedSubstream>;
    type Error = EitherError<<T as Transport>::Error, RelayError>;
    type Listener = RelayListener<T>;
    type ListenerUpgrade = RelayedListenerUpgrade<T>;
    type Dial = EitherFuture<<T as Transport>::Dial, RelayedDial>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let (is_relay, addr) = is_relay_listen_address(addr);
        if !is_relay {
            let inner_listener = match self.inner_transport.listen_on(addr) {
                Ok(listener) => listener,
                Err(TransportError::MultiaddrNotSupported(addr)) => {
                    return Err(TransportError::MultiaddrNotSupported(addr))
                }
                Err(TransportError::Other(err)) => {
                    return Err(TransportError::Other(EitherError::A(err)))
                }
            };
            return Ok(RelayListener {
                inner_listener: Some(inner_listener),
                // TODO: Do we want a listener for inner incoming connections to also yield relayed
                // connections?
                from_behaviour: self.from_behaviour.clone(),
                msg_to_behaviour: None,
            });
        }

        let mut to_behaviour = self.to_behaviour.clone();
        let (addr, peer_id) = split_off_peer_id(addr)?;
        let msg_to_behaviour = Some(
            async move {
                to_behaviour
                    .send(TransportToBehaviourMsg::ListenRequest {
                        address: addr,
                        peer_id,
                    })
                    .await
            }
            .boxed(),
        );

        Ok(RelayListener {
            inner_listener: None,
            from_behaviour: self.from_behaviour.clone(),
            msg_to_behaviour,
        })
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        if !contains_circuit_protocol(&addr) {
            match self.inner_transport.dial(addr) {
                Ok(dialer) => return Ok(EitherFuture::First(dialer)),
                Err(TransportError::MultiaddrNotSupported(addr)) => {
                    return Err(TransportError::MultiaddrNotSupported(addr))
                }
                Err(TransportError::Other(err)) => {
                    return Err(TransportError::Other(EitherError::A(err)))
                }
            };
        }

        let (relay, destination) = split_relay_and_destination(addr);
        let (relay_addr, relay_peer_id) = split_off_peer_id(relay)?;
        let (destination_addr, destination_peer_id) = split_off_peer_id(destination)?;

        let mut to_behaviour = self.to_behaviour.clone();
        Ok(EitherFuture::Second(
            async move {
                let (tx, rx) = oneshot::channel();
                to_behaviour
                    .send(TransportToBehaviourMsg::DialRequest {
                        relay_addr,
                        relay_peer_id,
                        destination_addr,
                        destination_peer_id,
                        send_back: tx,
                    })
                    .await?;
                let stream = rx.await?;
                Ok(stream)
            }
            .boxed(),
        ))
    }
}

fn contains_circuit_protocol(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| matches!(p, Protocol::P2pCircuit))
}

fn is_relay_listen_address(addr: Multiaddr) -> (bool, Multiaddr) {
    let original_len = addr.len();

    let new_addr: Multiaddr = addr
        .into_iter()
        .take_while(|p| !matches!(p, Protocol::P2pCircuit))
        .collect();

    (new_addr.len() != original_len, new_addr)
}

fn split_relay_and_destination(addr: Multiaddr) -> (Multiaddr, Multiaddr) {
    let mut relay = Vec::new();
    let mut destination = Vec::new();
    let mut passed_circuit = false;

    for protocol in addr.into_iter() {
        if matches!(protocol, Protocol::P2pCircuit) {
            passed_circuit = true;
            continue;
        }

        if !passed_circuit {
            relay.push(protocol);
        } else {
            destination.push(protocol);
        }
    }

    (
        relay.into_iter().collect(),
        destination.into_iter().collect(),
    )
}

fn split_off_peer_id(mut addr: Multiaddr) -> Result<(Multiaddr, PeerId), RelayError> {
    if let Some(Protocol::P2p(hash)) = addr.pop() {
        Ok((
            addr,
            PeerId::from_multihash(hash).map_err(|_| RelayError::InvalidHash)?,
        ))
    } else {
        Err(RelayError::MissingPeerId)
    }
}

#[pin_project]
pub struct RelayListener<T: Transport> {
    #[pin]
    inner_listener: Option<<T as Transport>::Listener>,
    from_behaviour: Arc<Mutex<mpsc::Receiver<BehaviourToTransportMsg>>>,

    msg_to_behaviour: Option<BoxFuture<'static, Result<(), mpsc::SendError>>>,
}

impl<T: Transport> Stream for RelayListener<T> {
    type Item = Result<
        ListenerEvent<RelayedListenerUpgrade<T>, EitherError<<T as Transport>::Error, RelayError>>,
        EitherError<<T as Transport>::Error, RelayError>,
    >;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        if let Some(msg) = this.msg_to_behaviour {
            match Future::poll(msg.as_mut(), cx) {
                Poll::Ready(Ok(())) => *this.msg_to_behaviour = None,
                Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(EitherError::B(e.into())))),
                Poll::Pending => {}
            }
        }

        if let Some(listener) = this.inner_listener.as_pin_mut() {
            match listener.poll_next(cx) {
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(EitherError::A(e)))),
                Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                    upgrade,
                    local_addr,
                    remote_addr,
                }))) => {
                    return Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                        upgrade: RelayedListenerUpgrade::Inner(upgrade),
                        local_addr,
                        remote_addr,
                    })))
                }
                Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr)))) => {
                    return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr))))
                }
                Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(addr)))) => {
                    return Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(addr))))
                }
                Poll::Ready(Some(Ok(ListenerEvent::Error(err)))) => {
                    return Poll::Ready(Some(Ok(ListenerEvent::Error(EitherError::A(err)))))
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => {}
            }
        }

        match this.from_behaviour.lock().unwrap().poll_next_unpin(cx) {
            Poll::Ready(Some(BehaviourToTransportMsg::IncomingRelayedConnection {
                stream,
                source,
            })) => {
                return Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                    upgrade: RelayedListenerUpgrade::Relayed(Some(stream)),
                    // TODO: Fix. Empty is not the right thing here. Should the address of the relay
                    // be mentioned here?
                    // Could one do this via IntoProtoHandler trait?
                    local_addr: Multiaddr::empty(),
                    remote_addr: Protocol::P2p(source.into()).into(),
                })));
            }
            Poll::Ready(None) => unimplemented!(),
            Poll::Pending => {}
        }

        Poll::Pending
    }
}

pub type RelayedDial = BoxFuture<'static, Result<NegotiatedSubstream, RelayError>>;

#[pin_project(project = RelayedListenerUpgradeProj)]
pub enum RelayedListenerUpgrade<T: Transport> {
    Inner(#[pin] <T as Transport>::ListenerUpgrade),
    Relayed(Option<NegotiatedSubstream>),
}

impl<T: Transport> Future for RelayedListenerUpgrade<T> {
    type Output = Result<
        EitherOutput<<T as Transport>::Output, NegotiatedSubstream>,
        EitherError<<T as Transport>::Error, RelayError>,
    >;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            RelayedListenerUpgradeProj::Inner(upgrade) => match upgrade.poll(cx) {
                Poll::Ready(Ok(out)) => return Poll::Ready(Ok(EitherOutput::First(out))),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(EitherError::A(err))),
                Poll::Pending => {}
            },
            RelayedListenerUpgradeProj::Relayed(substream) => {
                return Poll::Ready(Ok(EitherOutput::Second(substream.take().unwrap())))
            }
        }

        Poll::Pending
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RelayError {
    MissingPeerId,
    InvalidHash,
    FailedSendingMessageToBehaviour(mpsc::SendError),
    ResponseFromBehaviourCanceled,
}

impl<E> From<RelayError> for TransportError<EitherError<E, RelayError>> {
    fn from(error: RelayError) -> Self {
        TransportError::Other(EitherError::B(error))
    }
}

impl From<mpsc::SendError> for RelayError {
    fn from(error: mpsc::SendError) -> Self {
        RelayError::FailedSendingMessageToBehaviour(error)
    }
}

impl From<oneshot::Canceled> for RelayError {
    fn from(_: oneshot::Canceled) -> Self {
        RelayError::ResponseFromBehaviourCanceled
    }
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!();
    }
}

impl std::error::Error for RelayError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_off_peer_id() {
        let ip_address: Multiaddr = "/ip6/2001:db8::".parse().unwrap();
        assert_eq!(
            split_off_peer_id(ip_address.clone()),
            Err(RelayError::MissingPeerId)
        );

        let peer_id = PeerId::random();
        let ip_and_peer_id_address = ip_address
            .clone()
            .with(Protocol::P2p(peer_id.clone().into()));
        assert_eq!(
            split_off_peer_id(ip_and_peer_id_address.clone()),
            Ok((ip_address, peer_id))
        );
    }
}
