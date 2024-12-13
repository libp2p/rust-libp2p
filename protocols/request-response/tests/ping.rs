// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! Integration tests for the `Behaviour`.

use std::{io, iter};

use futures::prelude::*;
use libp2p_identity::PeerId;
use libp2p_request_response as request_response;
use libp2p_request_response::ProtocolSupport;
use libp2p_swarm::{StreamProtocol, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

#[async_std::test]
#[cfg(feature = "cbor")]
async fn is_response_outbound() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let ping = Ping("ping".to_string().into_bytes());
    let offline_peer = PeerId::random();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(
            [(
                StreamProtocol::new("/ping/1"),
                request_response::ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        )
    });

    let request_id1 = swarm1
        .behaviour_mut()
        .send_request(&offline_peer, ping.clone());

    match swarm1
        .next_swarm_event()
        .await
        .try_into_behaviour_event()
        .unwrap()
    {
        request_response::Event::OutboundFailure {
            peer,
            request_id: req_id,
            error: _error,
            ..
        } => {
            assert_eq!(&offline_peer, &peer);
            assert_eq!(req_id, request_id1);
        }
        e => panic!("Peer: Unexpected event: {e:?}"),
    }

    let request_id2 = swarm1.behaviour_mut().send_request(&offline_peer, ping);

    assert!(!swarm1
        .behaviour()
        .is_pending_outbound(&offline_peer, &request_id1));
    assert!(swarm1
        .behaviour()
        .is_pending_outbound(&offline_peer, &request_id2));
}

/// Exercises a simple ping protocol.
#[async_std::test]
#[cfg(feature = "cbor")]
async fn ping_protocol() {
    let ping = Ping("ping".to_string().into_bytes());
    let pong = Pong("pong".to_string().into_bytes());

    let protocols = iter::once((StreamProtocol::new("/ping/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();
    let mut swarm2 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols, cfg)
    });
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    let expected_ping = ping.clone();
    let expected_pong = pong.clone();

    let peer1 = async move {
        loop {
            match swarm1.next_swarm_event().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                }) => {
                    assert_eq!(&request, &expected_ping);
                    assert_eq!(&peer, &peer2_id);
                    swarm1
                        .behaviour_mut()
                        .send_response(channel, pong.clone())
                        .unwrap();
                }
                Ok(request_response::Event::ResponseSent { peer, .. }) => {
                    assert_eq!(&peer, &peer2_id);
                }
                Ok(e) => {
                    panic!("Peer1: Unexpected event: {e:?}")
                }
                Err(..) => {}
            }
        }
    };

    let num_pings: u8 = rand::thread_rng().gen_range(1..100);

    let peer2 = async {
        let mut count = 0;

        let mut req_id = swarm2.behaviour_mut().send_request(&peer1_id, ping.clone());
        assert!(swarm2.behaviour().is_pending_outbound(&peer1_id, &req_id));

        loop {
            match swarm2
                .next_swarm_event()
                .await
                .try_into_behaviour_event()
                .unwrap()
            {
                request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                } => {
                    count += 1;
                    assert_eq!(&response, &expected_pong);
                    assert_eq!(&peer, &peer1_id);
                    assert_eq!(req_id, request_id);
                    if count >= num_pings {
                        return;
                    } else {
                        req_id = swarm2.behaviour_mut().send_request(&peer1_id, ping.clone());
                    }
                }
                e => panic!("Peer2: Unexpected event: {e:?}"),
            }
        }
    };

    async_std::task::spawn(Box::pin(peer1));
    peer2.await;
}

#[async_std::test]
#[cfg(feature = "cbor")]
async fn emits_inbound_connection_closed_failure() {
    let ping = Ping("ping".to_string().into_bytes());

    let protocols = iter::once((StreamProtocol::new("/ping/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();
    let mut swarm2 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols, cfg)
    });
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    swarm2.behaviour_mut().send_request(&peer1_id, ping.clone());

    // Wait for swarm 1 to receive request by swarm 2.
    let _channel = loop {
        futures::select!(
            event = swarm1.select_next_some() => match event {
                SwarmEvent::Behaviour(request_response::Event::Message {
                    peer,
                    message: request_response::Message::Request { request, channel, .. },
                    ..
                }) => {
                    assert_eq!(&request, &ping);
                    assert_eq!(&peer, &peer2_id);
                    break channel;
                },
                SwarmEvent::Behaviour(ev) => panic!("Peer1: Unexpected event: {ev:?}"),
                _ => {}
            },
            event = swarm2.select_next_some() => {
                if let SwarmEvent::Behaviour(ev) = event {
                    panic!("Peer2: Unexpected event: {ev:?}");
                }
            }
        )
    };

    // Drop swarm 2 in order for the connection between swarm 1 and 2 to close.
    drop(swarm2);

    loop {
        match swarm1.select_next_some().await {
            SwarmEvent::Behaviour(request_response::Event::InboundFailure {
                error: request_response::InboundFailure::ConnectionClosed,
                ..
            }) => break,
            SwarmEvent::Behaviour(e) => panic!("Peer1: Unexpected event: {e:?}"),
            _ => {}
        }
    }
}

/// We expect the substream to be properly closed when response channel is dropped.
/// Since the ping protocol used here expects a response, the sender considers this
/// early close as a protocol violation which results in the connection being closed.
/// If the substream were not properly closed when dropped, the sender would instead
/// run into a timeout waiting for the response.
#[async_std::test]
#[cfg(feature = "cbor")]
async fn emits_inbound_connection_closed_if_channel_is_dropped() {
    let ping = Ping("ping".to_string().into_bytes());

    let protocols = iter::once((StreamProtocol::new("/ping/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let mut swarm1 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols.clone(), cfg.clone())
    });
    let peer1_id = *swarm1.local_peer_id();
    let mut swarm2 = Swarm::new_ephemeral(|_| {
        request_response::cbor::Behaviour::<Ping, Pong>::new(protocols, cfg)
    });
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().with_memory_addr_external().await;
    swarm2.connect(&mut swarm1).await;

    swarm2.behaviour_mut().send_request(&peer1_id, ping.clone());

    // Wait for swarm 1 to receive request by swarm 2.
    let event = loop {
        futures::select!(
            event = swarm1.select_next_some() => {
                if let SwarmEvent::Behaviour(request_response::Event::Message {
                    peer,
                    message: request_response::Message::Request { request, channel, .. },
                    ..
                }) = event {
                    assert_eq!(&request, &ping);
                    assert_eq!(&peer, &peer2_id);

                    drop(channel);
                    continue;
                }
            },
            event = swarm2.select_next_some() => {
                if let SwarmEvent::Behaviour(ev) = event {
                    break ev;
                }
            },
        )
    };

    let error = match event {
        request_response::Event::OutboundFailure { error, .. } => error,
        e => panic!("unexpected event from peer 2: {e:?}"),
    };

    assert!(matches!(
        error,
        request_response::OutboundFailure::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof,
    ));
}

// Simple Ping-Pong Protocol
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Ping(Vec<u8>);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pong(Vec<u8>);
