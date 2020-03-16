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

mod util;

use futures::prelude::*;
use libp2p_core::identity;
use libp2p_core::multiaddr::multiaddr;
use libp2p_core::{
    Network,
    PeerId,
    Transport,
    connection::PendingConnectionError,
    muxing::StreamMuxerBox,
    network::NetworkEvent,
    upgrade,
};
use libp2p_swarm::{
    NegotiatedSubstream,
    ProtocolsHandler,
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr,
    protocols_handler::NodeHandlerWrapperBuilder
};
use rand::seq::SliceRandom;
use std::{io, task::Context, task::Poll};

// TODO: replace with DummyProtocolsHandler after https://github.com/servo/rust-smallvec/issues/139 ?
#[derive(Default)]
struct TestHandler;

impl ProtocolsHandler for TestHandler {
    type InEvent = ();      // TODO: cannot be Void (https://github.com/servo/rust-smallvec/issues/139)
    type OutEvent = ();      // TODO: cannot be Void (https://github.com/servo/rust-smallvec/issues/139)
    type Error = io::Error;
    type InboundProtocol = upgrade::DeniedUpgrade;
    type OutboundProtocol = upgrade::DeniedUpgrade;
    type OutboundOpenInfo = ();      // TODO: cannot be Void (https://github.com/servo/rust-smallvec/issues/139)

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(upgrade::DeniedUpgrade)
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        _: <Self::InboundProtocol as upgrade::InboundUpgrade<NegotiatedSubstream>>::Output
    ) { panic!() }

    fn inject_fully_negotiated_outbound(
        &mut self,
        _: <Self::OutboundProtocol as upgrade::OutboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::OutboundOpenInfo
    ) { panic!() }

    fn inject_event(&mut self, _: Self::InEvent) {
        panic!()
    }

    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as upgrade::OutboundUpgrade<NegotiatedSubstream>>::Error>) {

    }

    fn connection_keep_alive(&self) -> KeepAlive { KeepAlive::No }

    fn poll(&mut self, _: &mut Context) -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>> {
        Poll::Pending
    }
}

#[test]
fn deny_incoming_connec() {
    // Checks whether refusing an incoming connection on a swarm triggers the correct events.

    let mut swarm1: Network<_, _, _, NodeHandlerWrapperBuilder<TestHandler>, _, _> = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new())
            .map(|(conn_info, muxer), _| (conn_info, StreamMuxerBox::new(muxer)));
        Network::new(transport, local_public_key.into(), Default::default())
    };

    let mut swarm2 = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new())
            .map(|(conn_info, muxer), _| (conn_info, StreamMuxerBox::new(muxer)));
        Network::new(transport, local_public_key.into(), Default::default())
    };

    swarm1.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let address = async_std::task::block_on(future::poll_fn(|cx| {
        if let Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) = swarm1.poll(cx) {
            Poll::Ready(listen_addr)
        } else {
            panic!("Was expecting the listen address to be reported")
        }
    }));

    swarm2
        .peer(swarm1.local_peer_id().clone())
        .into_disconnected().unwrap()
        .connect(address.clone(), Vec::new(), TestHandler::default().into_node_handler_builder())
        .unwrap();

    async_std::task::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        match swarm1.poll(cx) {
            Poll::Ready(NetworkEvent::IncomingConnection(inc)) => drop(inc),
            Poll::Ready(_) => unreachable!(),
            Poll::Pending => (),
        }

        match swarm2.poll(cx) {
            Poll::Ready(NetworkEvent::DialError {
                attempts_remaining: 0,
                peer_id,
                multiaddr,
                error: PendingConnectionError::Transport(_)
            }) => {
                assert_eq!(peer_id, *swarm1.local_peer_id());
                assert_eq!(multiaddr, address);
                return Poll::Ready(Ok(()));
            },
            Poll::Ready(_) => unreachable!(),
            Poll::Pending => (),
        }

        Poll::Pending
    })).unwrap();
}

#[test]
fn dial_self() {

    // Check whether dialing ourselves correctly fails.
    //
    // Dialing the same address we're listening should result in three events:
    //
    // - The incoming connection notification (before we know the incoming peer ID).
    // - The connection error for the dialing endpoint (once we've determined that it's our own ID).
    // - The connection error for the listening endpoint (once we've determined that it's our own ID).
    //
    // The last two can happen in any order.

    let mut swarm = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new())
            .and_then(|(peer, mplex), _| {
                // Gracefully close the connection to allow protocol
                // negotiation to complete.
                util::CloseMuxer::new(mplex).map_ok(move |mplex| (peer, mplex))
            })
            .map(|(conn_info, muxer), _| (conn_info, StreamMuxerBox::new(muxer)));
        Network::new(transport, local_public_key.into(), Default::default())
    };

    swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let (local_address, mut swarm) = async_std::task::block_on(
        future::lazy(move |cx| {
            if let Poll::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) = swarm.poll(cx) {
                Ok::<_, void::Void>((listen_addr, swarm))
            } else {
                panic!("Was expecting the listen address to be reported")
            }
        }))
        .unwrap();

    swarm.dial(&local_address, TestHandler::default().into_node_handler_builder()).unwrap();

    let mut got_dial_err = false;
    let mut got_inc_err = false;
    async_std::task::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        loop {
            match swarm.poll(cx) {
                Poll::Ready(NetworkEvent::UnknownPeerDialError {
                    multiaddr,
                    error: PendingConnectionError::InvalidPeerId { .. },
                    ..
                }) => {
                    assert!(!got_dial_err);
                    assert_eq!(multiaddr, local_address);
                    got_dial_err = true;
                    if got_inc_err {
                        return Poll::Ready(Ok(()))
                    }
                },
                Poll::Ready(NetworkEvent::IncomingConnectionError {
                    local_addr, ..
                }) => {
                    assert!(!got_inc_err);
                    assert_eq!(local_addr, local_address);
                    got_inc_err = true;
                    if got_dial_err {
                       return Poll::Ready(Ok(()))
                    }
                },
                Poll::Ready(NetworkEvent::IncomingConnection(inc)) => {
                    assert_eq!(*inc.local_addr(), local_address);
                    inc.accept(TestHandler::default().into_node_handler_builder()).unwrap();
                },
                Poll::Ready(ev) => {
                    panic!("Unexpected event: {:?}", ev)
                }
                Poll::Pending => break Poll::Pending,
            }
        }
    })).unwrap();
}

#[test]
fn dial_self_by_id() {
    // Trying to dial self by passing the same `PeerId` shouldn't even be possible in the first
    // place.

    let mut swarm: Network<_, _, _, NodeHandlerWrapperBuilder<TestHandler>> = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new())
            .map(|(conn_info, muxer), _| (conn_info, StreamMuxerBox::new(muxer)));
        Network::new(transport, local_public_key.into(), Default::default())
    };

    let peer_id = swarm.local_peer_id().clone();
    assert!(swarm.peer(peer_id).into_disconnected().is_none());
}

#[test]
fn multiple_addresses_err() {
    // Tries dialing multiple addresses, and makes sure there's one dialing error per addresses.

    let mut swarm = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new())
            .map(|(conn_info, muxer), _| (conn_info, StreamMuxerBox::new(muxer)));
        Network::new(transport, local_public_key.into(), Default::default())
    };

    let mut addresses = Vec::new();
    for _ in 0 .. 3 {
        addresses.push(multiaddr![Ip4([0, 0, 0, 0]), Tcp(rand::random::<u16>())]);
    }
    for _ in 0 .. 5 {
        addresses.push(multiaddr![Udp(rand::random::<u16>())]);
    }
    addresses.shuffle(&mut rand::thread_rng());

    let first = addresses[0].clone();
    let rest = (&addresses[1..]).iter().cloned();

    let target = PeerId::random();
    swarm.peer(target.clone())
        .into_disconnected().unwrap()
        .connect(first, rest, TestHandler::default().into_node_handler_builder())
        .unwrap();

    async_std::task::block_on(future::poll_fn(|cx| -> Poll<Result<(), io::Error>> {
        loop {
            match swarm.poll(cx) {
                Poll::Ready(NetworkEvent::DialError {
                    attempts_remaining,
                    peer_id,
                    multiaddr,
                    error: PendingConnectionError::Transport(_)
                }) => {
                    assert_eq!(peer_id, target);
                    let expected = addresses.remove(0);
                    assert_eq!(multiaddr, expected);
                    if addresses.is_empty() {
                        assert_eq!(attempts_remaining, 0);
                        return Poll::Ready(Ok(()));
                    } else {
                        assert_eq!(attempts_remaining, addresses.len());
                    }
                },
                Poll::Ready(_) => unreachable!(),
                Poll::Pending => break Poll::Pending,
            }
        }
    })).unwrap();
}
