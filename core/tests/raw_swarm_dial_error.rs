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
use libp2p_core::multiaddr::multiaddr;
use libp2p_core::nodes::raw_swarm::{RawSwarm, RawSwarmEvent, RawSwarmReachError, PeerState, UnknownPeerDialErr, IncomingError};
use libp2p_core::{PeerId, Transport, upgrade, upgrade::InboundUpgradeExt, upgrade::OutboundUpgradeExt};
use libp2p_core::protocols_handler::{ProtocolsHandler, KeepAlive, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, NodeHandlerWrapperBuilder};
use rand::seq::SliceRandom;
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
fn deny_incoming_connec() {
    // Checks whether refusing an incoming connection on a swarm triggers the correct events.

    // TODO: make creating the transport more elegant ; literaly half of the code of the test
    //       is about creating the transport
    let mut swarm1: RawSwarm<_, _, _, NodeHandlerWrapperBuilder<TestHandler<_>>, _> = {
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
        RawSwarm::new(transport, local_public_key.into())
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
        RawSwarm::new(transport, local_public_key.into())
    };

    let listen = swarm1.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    swarm2
        .peer(swarm1.local_peer_id().clone())
        .into_not_connected().unwrap()
        .connect(listen.clone(), TestHandler::default().into_node_handler_builder());

    let future = future::poll_fn(|| -> Poll<(), io::Error> {
        match swarm1.poll() {
            Async::Ready(RawSwarmEvent::IncomingConnection(inc)) => drop(inc),
            Async::Ready(_) => unreachable!(),
            Async::NotReady => (),
        }

        match swarm2.poll() {
            Async::Ready(RawSwarmEvent::DialError {
                new_state: PeerState::NotConnected,
                peer_id,
                multiaddr,
                error: RawSwarmReachError::Transport(_)
            }) => {
                assert_eq!(peer_id, *swarm1.local_peer_id());
                assert_eq!(multiaddr, listen);
                return Ok(Async::Ready(()));
            },
            Async::Ready(_) => unreachable!(),
            Async::NotReady => (),
        }

        Ok(Async::NotReady)
    });

    tokio::runtime::current_thread::Runtime::new().unwrap().block_on(future).unwrap();
}

#[test]
fn dial_self() {
    // Check whether dialing ourselves correctly fails.
    //
    // Dialing the same address we're listening should result in three events:
    //
    // - The incoming connection notification (before we know the incoming peer ID).
    // - The error about the incoming connection (once we've determined that it's our own ID).
    // - The error about the dialing (once we've determined that it's our own ID).
    //
    // The last two items can happen in any order.

    // TODO: make creating the transport more elegant ; literaly half of the code of the test
    //       is about creating the transport
    let mut swarm = {
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
        RawSwarm::new(transport, local_public_key.into())
    };

    let listen = swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
    swarm.dial(listen.clone(), TestHandler::default().into_node_handler_builder()).unwrap();

    let mut got_dial_err = false;
    let mut got_inc_err = false;
    let future = future::poll_fn(|| -> Poll<(), io::Error> {
        loop {
            match swarm.poll() {
                Async::Ready(RawSwarmEvent::UnknownPeerDialError {
                    multiaddr,
                    error: UnknownPeerDialErr::FoundLocalPeerId,
                    handler: _
                }) => {
                    assert_eq!(multiaddr, listen);
                    assert!(!got_dial_err);
                    got_dial_err = true;
                    if got_inc_err {
                        return Ok(Async::Ready(()));
                    }
                },
                Async::Ready(RawSwarmEvent::IncomingConnectionError {
                    listen_addr,
                    send_back_addr: _,
                    error: IncomingError::FoundLocalPeerId
                }) => {
                    assert_eq!(listen_addr, listen);
                    assert!(!got_inc_err);
                    got_inc_err = true;
                    if got_dial_err {
                        return Ok(Async::Ready(()));
                    }
                },
                Async::Ready(RawSwarmEvent::IncomingConnection(inc)) => {
                    assert_eq!(*inc.listen_addr(), listen);
                    inc.accept(TestHandler::default().into_node_handler_builder());
                },
                Async::Ready(ev) => unreachable!("{:?}", ev),
                Async::NotReady => break Ok(Async::NotReady),
            }
        }
    });

    tokio::runtime::current_thread::Runtime::new().unwrap().block_on(future).unwrap();
}

#[test]
fn dial_self_by_id() {
    // Trying to dial self by passing the same `PeerId` shouldn't even be possible in the first
    // place.

    // TODO: make creating the transport more elegant ; literaly half of the code of the test
    //       is about creating the transport
    let mut swarm: RawSwarm<_, _, _, NodeHandlerWrapperBuilder<TestHandler<_>>, _> = {
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
        RawSwarm::new(transport, local_public_key.into())
    };

    let peer_id = swarm.local_peer_id().clone();
    assert!(swarm.peer(peer_id).into_not_connected().is_none());
}

#[test]
fn multiple_addresses_err() {
    // Tries dialing multiple addresses, and makes sure there's one dialing error per addresses.

    // TODO: make creating the transport more elegant ; literaly half of the code of the test
    //       is about creating the transport
    let mut swarm = {
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
        RawSwarm::new(transport, local_public_key.into())
    };

    let mut addresses = Vec::new();
    for _ in 0 .. 3 {
        addresses.push(multiaddr![Ip4([0, 0, 0, 0]), Tcp(rand::random::<u16>())]);
    }
    for _ in 0 .. 5 {
        addresses.push(multiaddr![Udp(rand::random::<u16>())]);
    }
    addresses.shuffle(&mut rand::thread_rng());

    let target = PeerId::random();
    swarm.peer(target.clone())
        .into_not_connected().unwrap()
        .connect_iter(addresses.clone(), TestHandler::default().into_node_handler_builder())
        .unwrap();

    let future = future::poll_fn(|| -> Poll<(), io::Error> {
        loop {
            match swarm.poll() {
                Async::Ready(RawSwarmEvent::DialError {
                    new_state,
                    peer_id,
                    multiaddr,
                    error: RawSwarmReachError::Transport(_)
                }) => {
                    assert_eq!(peer_id, target);
                    let expected = addresses.remove(0);
                    assert_eq!(multiaddr, expected);
                    if addresses.is_empty() {
                        assert_eq!(new_state, PeerState::NotConnected);
                        return Ok(Async::Ready(()));
                    } else {
                        match new_state {
                            PeerState::Dialing { num_pending_addresses } => {
                                assert_eq!(num_pending_addresses.get(), addresses.len());
                            },
                            _ => panic!()
                        }
                    }
                },
                Async::Ready(_) => unreachable!(),
                Async::NotReady => break Ok(Async::NotReady),
            }
        }
    });

    tokio::runtime::current_thread::Runtime::new().unwrap().block_on(future).unwrap();
}
