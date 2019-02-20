// Copyright 2019 Parity Technologies (UK) Ltd.
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

use futures::{future, prelude::*};
use libp2p_core::identity;
use libp2p_core::nodes::raw_swarm::{RawSwarm, RawSwarmEvent, IncomingError};
use libp2p_core::{Transport, upgrade, upgrade::OutboundUpgradeExt, upgrade::InboundUpgradeExt};
use libp2p_core::protocols_handler::{ProtocolsHandler, KeepAlive, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr};
use std::{io, time::Duration, time::Instant};
use tokio_timer::Delay;

// TODO: replace with DummyProtocolsHandler after https://github.com/servo/rust-smallvec/issues/139 ?
struct TestHandler<TSubstream>(std::marker::PhantomData<TSubstream>, bool);

impl<TSubstream> Default for TestHandler<TSubstream> {
    fn default() -> Self {
        TestHandler(std::marker::PhantomData, false)
    }
}

impl<TSubstream> ProtocolsHandler for TestHandler<TSubstream>
where
    TSubstream: tokio_io::AsyncRead + tokio_io::AsyncWrite
{
    type InEvent = ();      // TODO: cannot be Void (https://github.com/servo/rust-smallvec/issues/139)
    type OutEvent = ();      // TODO: cannot be Void (https://github.com/servo/rust-smallvec/issues/139)
    type Error = io::Error;
    type Substream = TSubstream;
    type InboundProtocol = upgrade::DeniedUpgrade;
    type OutboundProtocol = upgrade::DeniedUpgrade;
    type OutboundOpenInfo = ();      // TODO: cannot be Void (https://github.com/servo/rust-smallvec/issues/139)

    fn listen_protocol(&self) -> Self::InboundProtocol {
        upgrade::DeniedUpgrade
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        _: <Self::InboundProtocol as upgrade::InboundUpgrade<Self::Substream>>::Output
    ) { panic!() }

    fn inject_fully_negotiated_outbound(
        &mut self,
        _: <Self::OutboundProtocol as upgrade::OutboundUpgrade<Self::Substream>>::Output,
        _: Self::OutboundOpenInfo
    ) { panic!() }

    fn inject_event(&mut self, _: Self::InEvent) {
        panic!()
    }

    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as upgrade::OutboundUpgrade<Self::Substream>>::Error>) {

    }

    fn inject_inbound_closed(&mut self) {}

    fn connection_keep_alive(&self) -> KeepAlive { KeepAlive::Now }

    fn shutdown(&mut self) { self.1 = true; }

    fn poll(&mut self) -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>, Self::Error> {
        if self.1 {
            Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown))
        } else {
            Ok(Async::NotReady)
        }
    }
}

#[test]
fn raw_swarm_simultaneous_connect() {
    // Checks whether two swarms dialing each other simultaneously properly works.

    // When two swarms A and B dial each other, the following can happen:
    //
    // - A and B both successfully open a dialing connection simultaneously, then either A or B
    //   (but not both) closes its dialing connection and get a `Replaced` event to replace the
    //   dialing connection with the listening one. The other one gets an `IncomingConnectionError`.
    // - A successfully dials B; B doesn't have dialing priority and thus cancels its dialing
    //   attempt. If A receives B's dialing attempt, it gets an `IncomingConnectionError`.
    // - A successfully dials B; B does have dialing priority and thus continues dialing; then B
    //   successfully dials A; A and B both get a `Replaced` event to replace the dialing
    //   connection with the listening one.
    //

    // Important note: This test is meant to detect race conditions which don't seem to happen
    //                 if we use the `MemoryTransport`. Using the TCP transport is important,
    //                 despite the fact that it adds a dependency.

    for _ in 0 .. 10 {
        // TODO: make creating the transport more elegant ; literaly half of the code of the test
        //       is about creating the transport
        let mut swarm1 = {
            let local_key = identity::Keypair::generate_ed25519();
            let local_public_key = local_key.public();
            let transport = libp2p_tcp::TcpConfig::new()
                .with_upgrade(libp2p_secio::SecioConfig::new(local_key))
                .and_then(move |out, endpoint| {
                    let peer_id = out.remote_key.into_peer_id();
                    let peer_id2 = peer_id.clone();
                    let upgrade = libp2p_mplex::MplexConfig::default()
                        .map_outbound(move |muxer| (peer_id, muxer))
                        .map_inbound(move |muxer| (peer_id2, muxer));
                    upgrade::apply(out.stream, upgrade, endpoint)
                });
            RawSwarm::new(transport, local_public_key.into_peer_id())
        };

        let mut swarm2 = {
            let local_key = identity::Keypair::generate_ed25519();
            let local_public_key = local_key.public();
            let transport = libp2p_tcp::TcpConfig::new()
                .with_upgrade(libp2p_secio::SecioConfig::new(local_key))
                .and_then(move |out, endpoint| {
                    let peer_id = out.remote_key.into_peer_id();
                    let peer_id2 = peer_id.clone();
                    let upgrade = libp2p_mplex::MplexConfig::default()
                        .map_outbound(move |muxer| (peer_id, muxer))
                        .map_inbound(move |muxer| (peer_id2, muxer));
                    upgrade::apply(out.stream, upgrade, endpoint)
                });
            RawSwarm::new(transport, local_public_key.into_peer_id())
        };

        let swarm1_listen = swarm1.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
        let swarm2_listen = swarm2.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

        let mut reactor = tokio::runtime::current_thread::Runtime::new().unwrap();

        for _ in 0 .. 10 {
            let mut swarm1_step = 0;
            let mut swarm2_step = 0;

            let mut swarm1_dial_start = Delay::new(Instant::now() + Duration::new(0, rand::random::<u32>() % 50_000_000));
            let mut swarm2_dial_start = Delay::new(Instant::now() + Duration::new(0, rand::random::<u32>() % 50_000_000));

            let future = future::poll_fn(|| -> Poll<(), io::Error> {
                loop {
                    let mut swarm1_not_ready = false;
                    let mut swarm2_not_ready = false;

                    // We add a lot of randomness. In a real-life situation the swarm also has to
                    // handle other nodes, which may delay the processing.

                    if swarm1_step == 0 {
                        match swarm1_dial_start.poll().unwrap() {
                            Async::Ready(_) => {
                                let handler = TestHandler::default().into_node_handler_builder();
                                swarm1.peer(swarm2.local_peer_id().clone()).into_not_connected().unwrap()
                                    .connect(swarm2_listen.clone(), handler);
                                swarm1_step = 1;
                                swarm1_not_ready = false;
                            },
                            Async::NotReady => swarm1_not_ready = true,
                        }
                    }

                    if swarm2_step == 0 {
                        match swarm2_dial_start.poll().unwrap() {
                            Async::Ready(_) => {
                                let handler = TestHandler::default().into_node_handler_builder();
                                swarm2.peer(swarm1.local_peer_id().clone()).into_not_connected().unwrap()
                                    .connect(swarm1_listen.clone(), handler);
                                swarm2_step = 1;
                                swarm2_not_ready = false;
                            },
                            Async::NotReady => swarm2_not_ready = true,
                        }
                    }

                    if rand::random::<f32>() < 0.1 {
                        match swarm1.poll() {
                            Async::Ready(RawSwarmEvent::IncomingConnectionError { error: IncomingError::DeniedLowerPriority, .. }) => {
                                assert_eq!(swarm1_step, 2);
                                swarm1_step = 3;
                            },
                            Async::Ready(RawSwarmEvent::Connected { peer_id, .. }) => {
                                assert_eq!(peer_id, *swarm2.local_peer_id());
                                assert_eq!(swarm1_step, 1);
                                swarm1_step = 2;
                            },
                            Async::Ready(RawSwarmEvent::Replaced { peer_id, .. }) => {
                                assert_eq!(peer_id, *swarm2.local_peer_id());
                                assert_eq!(swarm1_step, 2);
                                swarm1_step = 3;
                            },
                            Async::Ready(RawSwarmEvent::IncomingConnection(inc)) => {
                                inc.accept(TestHandler::default().into_node_handler_builder());
                            },
                            Async::Ready(_) => unreachable!(),
                            Async::NotReady => swarm1_not_ready = true,
                        }
                    }

                    if rand::random::<f32>() < 0.1 {
                        match swarm2.poll() {
                            Async::Ready(RawSwarmEvent::IncomingConnectionError { error: IncomingError::DeniedLowerPriority, .. }) => {
                                assert_eq!(swarm2_step, 2);
                                swarm2_step = 3;
                            },
                            Async::Ready(RawSwarmEvent::Connected { peer_id, .. }) => {
                                assert_eq!(peer_id, *swarm1.local_peer_id());
                                assert_eq!(swarm2_step, 1);
                                swarm2_step = 2;
                            },
                            Async::Ready(RawSwarmEvent::Replaced { peer_id, .. }) => {
                                assert_eq!(peer_id, *swarm1.local_peer_id());
                                assert_eq!(swarm2_step, 2);
                                swarm2_step = 3;
                            },
                            Async::Ready(RawSwarmEvent::IncomingConnection(inc)) => {
                                inc.accept(TestHandler::default().into_node_handler_builder());
                            },
                            Async::Ready(_) => unreachable!(),
                            Async::NotReady => swarm2_not_ready = true,
                        }
                    }

                    // TODO: make sure that >= 5 is correct
                    if swarm1_step + swarm2_step >= 5 {
                        return Ok(Async::Ready(()));
                    }

                    if swarm1_not_ready && swarm2_not_ready {
                        return Ok(Async::NotReady);
                    }
                }
            });

            reactor.block_on(future).unwrap();

            // We now disconnect them again.
            swarm1.peer(swarm2.local_peer_id().clone()).into_connected().unwrap().close();
            swarm2.peer(swarm1.local_peer_id().clone()).into_connected().unwrap().close();
        }
    }
}
