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
use libp2p_core::nodes::raw_swarm::{RawSwarm, RawSwarmEvent, IncomingError};
use libp2p_core::{Transport, upgrade, upgrade::OutboundUpgradeExt};
use libp2p_core::protocols_handler::{ProtocolsHandler, KeepAlive, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr};
use std::io;

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

    // TODO: make creating the transport more elegant ; literaly half of the code of the test
    //       is about creating the transport
    let mut swarm1 = {
        let local_key = libp2p_secio::SecioKeyPair::ed25519_generated().unwrap();
        let local_public_key = local_key.to_public_key();
        let transport = libp2p_tcp::TcpConfig::new()
            .with_upgrade(libp2p_secio::SecioConfig::new(local_key))
            .and_then(move |out, _| {
                let peer_id = out.remote_key.into_peer_id();
                let upgrade =
                    libp2p_mplex::MplexConfig::new().map_outbound(move |muxer| (peer_id, muxer));
                upgrade::apply_outbound(out.stream, upgrade)
            });
        RawSwarm::new(transport, local_public_key.into_peer_id())
    };

    let mut swarm2 = {
        let local_key = libp2p_secio::SecioKeyPair::ed25519_generated().unwrap();
        let local_public_key = local_key.to_public_key();
        let transport = libp2p_tcp::TcpConfig::new()
            .with_upgrade(libp2p_secio::SecioConfig::new(local_key))
            .and_then(move |out, _| {
                let peer_id = out.remote_key.into_peer_id();
                let upgrade =
                    libp2p_mplex::MplexConfig::new().map_outbound(move |muxer| (peer_id, muxer));
                upgrade::apply_outbound(out.stream, upgrade)
            });
        RawSwarm::new(transport, local_public_key.into_peer_id())
    };

    let swarm1_listen = swarm1.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
    let swarm2_listen = swarm2.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let mut reactor = tokio::runtime::current_thread::Runtime::new().unwrap();

    for _ in 0 .. 100 {
        let handler1 = TestHandler::default().into_node_handler_builder();
        let handler2 = TestHandler::default().into_node_handler_builder();

        swarm1.peer(swarm2.local_peer_id().clone()).into_not_connected().unwrap()
            .connect(swarm2_listen.clone(), handler1);
        swarm2.peer(swarm1.local_peer_id().clone()).into_not_connected().unwrap()
            .connect(swarm1_listen.clone(), handler2);

        let mut swarm1_step = 0;
        let mut swarm2_step = 0;

        let future = future::poll_fn(|| -> Poll<(), io::Error> {
            loop {
                let mut swarm1_not_ready = false;
                match swarm1.poll() {
                    Async::Ready(RawSwarmEvent::IncomingConnectionError { error: IncomingError::DeniedLowerPriority, .. }) => {
                        assert_eq!(swarm1_step, 1);
                        swarm1_step = 2;
                    },
                    Async::Ready(RawSwarmEvent::Connected { peer_id, .. }) => {
                        assert_eq!(peer_id, *swarm2.local_peer_id());
                        assert_eq!(swarm1_step, 0);
                        swarm1_step = 1;
                    },
                    Async::Ready(RawSwarmEvent::Replaced { peer_id, .. }) => {
                        assert_eq!(peer_id, *swarm2.local_peer_id());
                        assert_eq!(swarm1_step, 1);
                        swarm1_step = 2;
                    },
                    Async::Ready(RawSwarmEvent::IncomingConnection(inc)) => {
                        inc.accept(TestHandler::default().into_node_handler_builder());
                    },
                    Async::Ready(_) => unreachable!(),
                    Async::NotReady => { swarm1_not_ready = true; }
                }

                match swarm2.poll() {
                    Async::Ready(RawSwarmEvent::IncomingConnectionError { error: IncomingError::DeniedLowerPriority, .. }) => {
                        assert_eq!(swarm2_step, 1);
                        swarm2_step = 2;
                    },
                    Async::Ready(RawSwarmEvent::Connected { peer_id, .. }) => {
                        assert_eq!(peer_id, *swarm1.local_peer_id());
                        assert_eq!(swarm2_step, 0);
                        swarm2_step = 1;
                    },
                    Async::Ready(RawSwarmEvent::Replaced { peer_id, .. }) => {
                        assert_eq!(peer_id, *swarm1.local_peer_id());
                        assert_eq!(swarm2_step, 1);
                        swarm2_step = 2;
                    },
                    Async::Ready(RawSwarmEvent::IncomingConnection(inc)) => {
                        inc.accept(TestHandler::default().into_node_handler_builder());
                    },
                    Async::Ready(_) => unreachable!(),
                    Async::NotReady if swarm1_not_ready => return Ok(Async::NotReady),
                    Async::NotReady => (),
                }

                if swarm1_step == 2 && swarm2_step == 2 {
                    return Ok(Async::Ready(()));
                }
            }
        });

        reactor.block_on(future).unwrap();

        // We now disconnect them again.
        swarm1.peer(swarm2.local_peer_id().clone()).into_connected().unwrap().close();
        swarm2.peer(swarm1.local_peer_id().clone()).into_connected().unwrap().close();
    }
}
