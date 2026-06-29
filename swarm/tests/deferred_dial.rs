use std::{
    task::{Context, Poll},
    time::Duration,
};

use futures::{FutureExt, channel::oneshot};
use libp2p_core::{
    Endpoint, Multiaddr, Transport,
    transport::{PortUse, dummy::DummyTransport},
};
use libp2p_identity::{Keypair, PeerId};
use libp2p_swarm::{
    Config, ConnectionDenied, ConnectionId, DialError, FromSwarm, NetworkBehaviour,
    OutboundAddresses, Swarm, SwarmEvent, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    dial_opts::DialOpts, dummy,
};
use libp2p_swarm_test::SwarmExt;

/// A [`NetworkBehaviour`] that resolves the addresses for an outbound dial **asynchronously**
/// ([`OutboundAddresses::Pending`]), so the dial always takes the deferred, `Swarm::poll`-driven
/// path rather than the synchronous fast-path.
///
/// If `gate` is set, the resolution future blocks until the gate is released, which lets a test
/// abort the dial mid-resolution.
struct AsyncAddrBehaviour {
    addr: Multiaddr,
    gate: Option<oneshot::Receiver<()>>,
}

impl NetworkBehaviour for AsyncAddrBehaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = std::convert::Infallible;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
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

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> OutboundAddresses {
        let addr = self.addr.clone();
        let gate = self.gate.take();

        OutboundAddresses::Pending(
            async move {
                match gate {
                    // Block until the test releases the gate.
                    Some(gate) => {
                        let _ = gate.await;
                    }
                    // Yield once so the deferred path is still exercised (never `Ready` inline).
                    None => tokio::task::yield_now().await,
                }
                Ok(vec![addr])
            }
            .boxed(),
        )
    }

    fn on_swarm_event(&mut self, _: FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        // `dummy::ConnectionHandler` never emits an event (`ToBehaviour = Infallible`).
        match event {}
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

/// Dialing by `PeerId` only, with the address supplied asynchronously by the behaviour, still
/// establishes the connection, exercising the deferred (`Pending`) address-resolution path.
#[tokio::test]
async fn deferred_outbound_address_resolution_dials_resolved_address() {
    let mut responder = Swarm::new_ephemeral_tokio(|_| dummy::Behaviour);
    let (responder_addr, _) = responder.listen().with_memory_addr_external().await;
    let responder_peer = *responder.local_peer_id();

    let mut dialer = Swarm::new_ephemeral_tokio(|_| AsyncAddrBehaviour {
        addr: responder_addr,
        gate: None,
    });

    dialer
        .dial(
            DialOpts::peer_id(responder_peer)
                .addresses(vec![])
                .extend_addresses_through_behaviour()
                .build(),
        )
        .expect("`Swarm::dial` to accept the deferred dial");

    loop {
        tokio::select! {
            _ = responder.next_swarm_event() => {}
            event = dialer.next_swarm_event() => match event {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    assert_eq!(peer_id, responder_peer);
                    break;
                }
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    panic!("deferred dial failed: {error:?}");
                }
                _ => {}
            }
        }
    }
}

/// Aborting a peer (`disconnect_peer_id`) while its dial addresses are still being resolved
/// abandons the dial: no connection is established and no error is surfaced, even after the
/// resolution future eventually completes.
#[tokio::test]
async fn aborting_during_resolution_abandons_the_dial() {
    let mut responder = Swarm::new_ephemeral_tokio(|_| dummy::Behaviour);
    let (responder_addr, _) = responder.listen().with_memory_addr_external().await;
    let responder_peer = *responder.local_peer_id();

    let (release_gate, gate) = oneshot::channel();
    let mut dialer = Swarm::new_ephemeral_tokio(|_| AsyncAddrBehaviour {
        addr: responder_addr,
        gate: Some(gate),
    });

    dialer
        .dial(
            DialOpts::peer_id(responder_peer)
                .addresses(vec![])
                .extend_addresses_through_behaviour()
                .build(),
        )
        .expect("`Swarm::dial` to accept the deferred dial");

    // Abort the dial while resolution is still gated, then let resolution complete.
    let _ = dialer.disconnect_peer_id(responder_peer);
    release_gate.send(()).unwrap();

    // Drive both swarms: the resolved addresses must be discarded, so no terminal event for the
    // responder should ever arrive. We expect the timeout to elapse.
    let outcome = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            tokio::select! {
                _ = responder.next_swarm_event() => {}
                event = dialer.next_swarm_event() => match event {
                    SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == responder_peer => {
                        return "connected";
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id: Some(p), .. } if p == responder_peer => {
                        return "errored";
                    }
                    _ => {}
                }
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "aborted dial should produce neither a connection nor an error, got {outcome:?}"
    );
}

/// A resolution that never completes is failed once the configured
/// [`Config::with_outbound_address_resolution_timeout`] elapses.
#[tokio::test]
async fn outbound_address_resolution_times_out() {
    let keypair = Keypair::generate_ed25519();
    let local_peer_id = keypair.public().to_peer_id();

    // Hold the sender for the whole test so the gate is never released: resolution never completes.
    let (_release_gate, gate) = oneshot::channel();

    let mut dialer = Swarm::new(
        DummyTransport::new().boxed(),
        AsyncAddrBehaviour {
            addr: "/memory/1".parse().unwrap(),
            gate: Some(gate),
        },
        local_peer_id,
        Config::with_tokio_executor()
            .with_outbound_address_resolution_timeout(Duration::from_millis(100)),
    );

    let target = PeerId::random();
    dialer
        .dial(
            DialOpts::peer_id(target)
                .addresses(vec![])
                .extend_addresses_through_behaviour()
                .build(),
        )
        .expect("`Swarm::dial` to accept the deferred dial");

    let (peer_id, error) = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SwarmEvent::OutgoingConnectionError { peer_id, error, .. } =
                dialer.next_swarm_event().await
            {
                return (peer_id, error);
            }
        }
    })
    .await
    .expect("the resolution timeout should fail the dial");

    assert_eq!(peer_id, Some(target));
    assert!(
        matches!(error, DialError::Denied { .. }),
        "expected `DialError::Denied` from the resolution timeout, got {error:?}"
    );
}
