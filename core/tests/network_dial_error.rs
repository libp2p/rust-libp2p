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

use futures::{future, prelude::*};
use libp2p_core::identity;
use libp2p_core::multiaddr::multiaddr;
use libp2p_core::nodes::network::{Network, NetworkEvent, NetworkReachError, PeerState, UnknownPeerDialErr, IncomingError};
use libp2p_core::{PeerId, Transport, upgrade};
use libp2p_swarm::{
    ProtocolsHandler,
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr,
    protocols_handler::NodeHandlerWrapperBuilder
};
use rand::seq::SliceRandom;
use std::io;

// TODO: replace with DummyProtocolsHandler after https://github.com/servo/rust-smallvec/issues/139 ?
struct TestHandler<TSubstream>(std::marker::PhantomData<TSubstream>);

impl<TSubstream> Default for TestHandler<TSubstream> {
    fn default() -> Self {
        TestHandler(std::marker::PhantomData)
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

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(upgrade::DeniedUpgrade)
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

    fn connection_keep_alive(&self) -> KeepAlive { KeepAlive::No }

    fn poll(&mut self) -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>, Self::Error> {
        Ok(Async::NotReady)
    }
}

#[test]
fn deny_incoming_connec() {
    // Checks whether refusing an incoming connection on a swarm triggers the correct events.

    let mut swarm1: Network<_, _, _, NodeHandlerWrapperBuilder<TestHandler<_>>, _> = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new());
        Network::new(transport, local_public_key.into())
    };

    let mut swarm2 = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new());
        Network::new(transport, local_public_key.into())
    };

    swarm1.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let address =
        if let Async::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) = swarm1.poll() {
            listen_addr
        } else {
            panic!("Was expecting the listen address to be reported")
        };

    swarm2
        .peer(swarm1.local_peer_id().clone())
        .into_not_connected().unwrap()
        .connect(address.clone(), TestHandler::default().into_node_handler_builder());

    let future = future::poll_fn(|| -> Poll<(), io::Error> {
        match swarm1.poll() {
            Async::Ready(NetworkEvent::IncomingConnection(inc)) => drop(inc),
            Async::Ready(_) => unreachable!(),
            Async::NotReady => (),
        }

        match swarm2.poll() {
            Async::Ready(NetworkEvent::DialError {
                new_state: PeerState::NotConnected,
                peer_id,
                multiaddr,
                error: NetworkReachError::Transport(_)
            }) => {
                assert_eq!(peer_id, *swarm1.local_peer_id());
                assert_eq!(multiaddr, address);
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
                util::CloseMuxer::new(mplex).map(move |mplex| (peer, mplex))
            });
        Network::new(transport, local_public_key.into())
    };

    swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

    let (address, mut swarm) =
        future::lazy(move || {
            if let Async::Ready(NetworkEvent::NewListenerAddress { listen_addr, .. }) = swarm.poll() {
                Ok::<_, void::Void>((listen_addr, swarm))
            } else {
                panic!("Was expecting the listen address to be reported")
            }
        })
        .wait()
        .unwrap();

    swarm.dial(address.clone(), TestHandler::default().into_node_handler_builder()).unwrap();

    let mut got_dial_err = false;
    let mut got_inc_err = false;
    let future = future::poll_fn(|| -> Poll<(), io::Error> {
        loop {
            match swarm.poll() {
                Async::Ready(NetworkEvent::UnknownPeerDialError {
                    multiaddr,
                    error: UnknownPeerDialErr::FoundLocalPeerId,
                    handler: _
                }) => {
                    assert_eq!(multiaddr, address);
                    assert!(!got_dial_err);
                    got_dial_err = true;
                    if got_inc_err {
                        return Ok(Async::Ready(()));
                    }
                },
                Async::Ready(NetworkEvent::IncomingConnectionError {
                    local_addr,
                    send_back_addr: _,
                    error: IncomingError::FoundLocalPeerId
                }) => {
                    assert_eq!(address, local_addr);
                    assert!(!got_inc_err);
                    got_inc_err = true;
                    if got_dial_err {
                        return Ok(Async::Ready(()));
                    }
                },
                Async::Ready(NetworkEvent::IncomingConnection(inc)) => {
                    assert_eq!(*inc.local_addr(), address);
                    inc.accept(TestHandler::default().into_node_handler_builder());
                },
                Async::Ready(ev) => {
                    panic!("Unexpected event: {:?}", ev)
                }
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

    let mut swarm: Network<_, _, _, NodeHandlerWrapperBuilder<TestHandler<_>>, _> = {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p_secio::SecioConfig::new(local_key))
            .multiplex(libp2p_mplex::MplexConfig::new());
        Network::new(transport, local_public_key.into())
    };

    let peer_id = swarm.local_peer_id().clone();
    assert!(swarm.peer(peer_id).into_not_connected().is_none());
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
            .multiplex(libp2p_mplex::MplexConfig::new());
        Network::new(transport, local_public_key.into())
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
                Async::Ready(NetworkEvent::DialError {
                    new_state,
                    peer_id,
                    multiaddr,
                    error: NetworkReachError::Transport(_)
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
