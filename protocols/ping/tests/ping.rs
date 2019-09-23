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

//! Integration tests for the `Ping` network behaviour.

use libp2p_core::{
    Multiaddr,
    PeerId,
    Negotiated,
    identity,
    transport::{Transport, boxed::Boxed},
    either::EitherError,
    upgrade::{self, UpgradeError}
};
use libp2p_ping::*;
use libp2p_yamux::{self as yamux, Yamux};
use libp2p_secio::{SecioConfig, SecioOutput, SecioError};
use libp2p_swarm::Swarm;
use libp2p_tcp::{TcpConfig, TcpTransStream};
use futures::{future, prelude::*};
use std::{io, time::Duration, sync::mpsc::sync_channel};
use tokio::runtime::Runtime;

#[test]
fn ping() {
    let cfg = PingConfig::new().with_keep_alive(true);

    let (peer1_id, trans) = mk_transport();
    let mut swarm1 = Swarm::new(trans, Ping::new(cfg.clone()), peer1_id.clone());

    let (peer2_id, trans) = mk_transport();
    let mut swarm2 = Swarm::new(trans, Ping::new(cfg), peer2_id.clone());

    let (tx, rx) = sync_channel::<Multiaddr>(1);

    let pid1 = peer1_id.clone();
    let addr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    let mut listening = false;
    Swarm::listen_on(&mut swarm1, addr).unwrap();
    let peer1 = future::poll_fn(move || -> Result<_, ()> {
        loop {
            match swarm1.poll().expect("Error while polling swarm") {
                Async::Ready(Some(PingEvent { peer, result })) => match result {
                    Ok(PingSuccess::Ping { rtt }) =>
                        return Ok(Async::Ready((pid1.clone(), peer, rtt))),
                    _ => {}
                },
                _ => {
                    if !listening {
                        for l in Swarm::listeners(&swarm1) {
                            tx.send(l.clone()).unwrap();
                            listening = true;
                        }
                    }
                    return Ok(Async::NotReady)
                }
            }
        }
    });

    let pid2 = peer2_id.clone();
    let mut dialing = false;
    let peer2 = future::poll_fn(move || -> Result<_, ()> {
        loop {
            match swarm2.poll().expect("Error while polling swarm") {
                Async::Ready(Some(PingEvent { peer, result })) => match result {
                    Ok(PingSuccess::Ping { rtt }) =>
                        return Ok(Async::Ready((pid2.clone(), peer, rtt))),
                    _ => {}
                },
                _ => {
                    if !dialing {
                        Swarm::dial_addr(&mut swarm2, rx.recv().unwrap()).unwrap();
                        dialing = true;
                    }
                    return Ok(Async::NotReady)
                }
            }
        }
    });

    let result = peer1.select(peer2).map_err(|e| panic!(e));
    let ((p1, p2, rtt), _) = Runtime::new().unwrap().block_on(result).unwrap();
    assert!(p1 == peer1_id && p2 == peer2_id || p1 == peer2_id && p2 == peer1_id);
    assert!(rtt < Duration::from_millis(50));
}

fn mk_transport() -> (
    PeerId,
    Boxed<
        (PeerId, Yamux<Negotiated<SecioOutput<Negotiated<TcpTransStream>>>>),
        EitherError<EitherError<io::Error, UpgradeError<SecioError>>, UpgradeError<io::Error>>
    >
) {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().into_peer_id();
    let transport = TcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        .authenticate(SecioConfig::new(id_keys))
        .multiplex(yamux::Config::default())
        .boxed();
    (peer_id, transport)
}

