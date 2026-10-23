// Copyright 2026 LimeChain
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

//! Regression test for the `on_connection_closed` connection-tracking panic
//! (`assertion left == right failed, left: false, right: true`).
//!
//! `Behaviour` records a connection in its `connected` map inside
//! `handle_established_*_connection` (handler-creation time) but ignores
//! `FromSwarm::ConnectionEstablished`. In a *composed* behaviour the swarm calls every sibling's
//! `handle_established_*` in sequence; if a later sibling returns `ConnectionDenied`, this
//! behaviour has already recorded the connection, yet it never enters the swarm's connection pool
//! -- so it is never counted in `remaining_established` and never produces its own
//! `ConnectionClosed`. It lingers as a phantom entry. When the last *real* connection to the peer
//! closes with `remaining_established == 0`, the phantom is still tracked, which used to trip a
//! `debug_assert_eq!(connections.is_empty(), remaining_established == 0)` and panic the node.

use std::{
    io,
    task::{Context, Poll},
};

use futures::prelude::*;
use libp2p_core::{ConnectedPoint, Endpoint, Multiaddr, transport::PortUse};
use libp2p_identity::PeerId;
use libp2p_request_response as request_response;
use libp2p_request_response::{Codec, ProtocolSupport};
use libp2p_swarm::{
    ConnectionId, NetworkBehaviour, StreamProtocol, ToSwarm,
    behaviour::{ConnectionClosed, ConnectionEstablished, FromSwarm},
};

#[derive(Clone, Default)]
struct DummyCodec;

impl Codec for DummyCodec {
    type Protocol = StreamProtocol;
    type Request = ();
    type Response = ();

    async fn read_request<T>(&mut self, _: &Self::Protocol, _: &mut T) -> io::Result<()>
    where
        T: AsyncRead + Unpin + Send,
    {
        Ok(())
    }
    async fn read_response<T>(&mut self, _: &Self::Protocol, _: &mut T) -> io::Result<()>
    where
        T: AsyncRead + Unpin + Send,
    {
        Ok(())
    }
    async fn write_request<T>(&mut self, _: &Self::Protocol, _: &mut T, _: ()) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        Ok(())
    }
    async fn write_response<T>(&mut self, _: &Self::Protocol, _: &mut T, _: ()) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        Ok(())
    }
}

fn behaviour() -> request_response::Behaviour<DummyCodec> {
    request_response::Behaviour::with_codec(
        DummyCodec,
        std::iter::once((StreamProtocol::new("/test/1"), ProtocolSupport::Full)),
        request_response::Config::default(),
    )
}

fn dialer_endpoint() -> ConnectedPoint {
    ConnectedPoint::Dialer {
        address: "/ip4/127.0.0.1/tcp/1".parse().unwrap(),
        role_override: Endpoint::Dialer,
        port_use: PortUse::Reuse,
    }
}

/// A connection the swarm hands to `handle_established_*` but that a sibling behaviour then denies
/// never yields a `FromSwarm::ConnectionEstablished`, so it must never be recorded. When the one
/// real connection then closes with `remaining_established == 0`, tracking must be exactly empty --
/// no phantom left behind.
///
/// Before the fix (which recorded at `handle_established_*`) the phantom lingered and closing the
/// real connection tripped `debug_assert_eq!(connections.is_empty(), remaining_established == 0)`.
#[test]
fn denied_connection_leaves_no_phantom_on_last_close() {
    let mut behaviour = behaviour();
    let peer = PeerId::random();
    let addr: Multiaddr = "/ip4/127.0.0.1/tcp/10000".parse().unwrap();
    let endpoint = dialer_endpoint();

    let phantom = ConnectionId::new_unchecked(1);
    let real = ConnectionId::new_unchecked(2);

    // The swarm speculatively asks every behaviour for a handler before admitting the connection.
    // `phantom` is the one a sibling denies: it never enters the pool, so no
    // `ConnectionEstablished` (nor `ConnectionClosed`) ever follows for it.
    behaviour
        .handle_established_inbound_connection(phantom, peer, &addr, &addr)
        .unwrap();
    behaviour
        .handle_established_inbound_connection(real, peer, &addr, &addr)
        .unwrap();

    // Only the real connection is admitted to the pool.
    behaviour.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
        peer_id: peer,
        connection_id: real,
        endpoint: &endpoint,
        failed_addresses: &[],
        other_established: 0,
    }));

    // It then closes as the last established connection. With no phantom recorded, tracking is
    // exactly empty and the invariant holds -- must not panic.
    behaviour.on_swarm_event(FromSwarm::ConnectionClosed(ConnectionClosed {
        peer_id: peer,
        connection_id: real,
        endpoint: &endpoint,
        cause: None,
        remaining_established: 0,
    }));
}

/// The correctness point behind the panic: a denied connection must not be recorded, because a
/// phantom entry makes the peer look connected and routes outbound requests to a dead
/// `connection_id` instead of dialing. After only a denied `handle_established_*` (no
/// `ConnectionEstablished`), a `send_request` must dial the peer, not notify a phantom handler.
#[test]
fn denied_connection_is_not_routable() {
    let mut behaviour = behaviour();
    let peer = PeerId::random();
    let addr: Multiaddr = "/ip4/127.0.0.1/tcp/1".parse().unwrap();

    // A connection handed to the behaviour but denied by a sibling: `handle_established` runs, but
    // no `ConnectionEstablished` follows.
    behaviour
        .handle_established_inbound_connection(ConnectionId::new_unchecked(1), peer, &addr, &addr)
        .unwrap();

    behaviour.send_request(&peer, ());

    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    match behaviour.poll(&mut cx) {
        // Correct: the peer is not connected, so the behaviour dials it.
        Poll::Ready(ToSwarm::Dial { .. }) => {}
        Poll::Ready(ToSwarm::NotifyHandler { .. }) => {
            panic!("request was routed to a phantom connection instead of dialing the peer")
        }
        _ => panic!("expected the behaviour to dial the peer for the queued request"),
    }
}
