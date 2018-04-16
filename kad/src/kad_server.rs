// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Contains a `ConnectionUpgrade` that makes it possible to send requests and receive responses
//! from nodes after the upgrade.
//!
//! # Usage
//!
//! - Implement the `KadServerInterface` trait on something clonable (usually an `Arc`).
//!
//! - Create a `KademliaServerConfig` object from that interface. This struct implements
//!   `ConnectionUpgrade`.
//!
//! - Update a connection through that `KademliaServerConfig`. The output yields you a
//!   `KademliaServerController` and a future that must be driven to completion. The controller
//!   allows you to perform queries and receive responses.
//!
//! This `KademliaServerController` is usually extracted and stored in some sort of hash map in an
//! `Arc` in order to be available whenever we need to request something from a node.

use bytes::Bytes;
use futures::{future, Future, Sink, Stream};
use futures::sync::{mpsc, oneshot};
use libp2p_peerstore::PeerId;
use libp2p_swarm::ConnectionUpgrade;
use libp2p_swarm::Endpoint;
use multiaddr::{AddrComponent, Multiaddr};
use protocol::{self, KadMsg, KademliaProtocolConfig, Peer};
use std::collections::VecDeque;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};

/// Interface that this server system uses to communicate with the rest of the system.
pub trait KadServerInterface: Clone {
    /// Returns the peer ID of the local node.
    fn local_id(&self) -> &PeerId;

    /// Returns known information about the peer. Not atomic/thread-safe in the sense that
    /// information can change immediately after being returned and before they are processed.
    fn peer_info(&self, _: &PeerId) -> (Vec<Multiaddr>, protocol::ConnectionType);

    /// Updates an entry in the K-Buckets. Called whenever that peer sends us a message.
    fn kbuckets_update(&self, peer: &PeerId);

    /// Finds the nodes closest to a peer ID.
    fn kbuckets_find_closest(&self, addr: &PeerId) -> Vec<PeerId>;
}

/// Configuration for a Kademlia server.
///
/// Implements `ConnectionUpgrade`. On a successful upgrade, produces a `KademliaServerController`
/// and a `Future`. The controller lets you send queries to the remote and receive answers, while
/// the `Future` must be driven to completion in order for things to work.
#[derive(Debug, Clone)]
pub struct KademliaServerConfig<I> {
    raw_proto: KademliaProtocolConfig,
    interface: I,
}

impl<I> KademliaServerConfig<I> {
    /// Builds a configuration object for an upcoming Kademlia server.
    #[inline]
    pub fn new(interface: I) -> Self {
        KademliaServerConfig {
            raw_proto: KademliaProtocolConfig,
            interface: interface,
        }
    }
}

impl<C, I> ConnectionUpgrade<C> for KademliaServerConfig<I>
where
    C: AsyncRead + AsyncWrite + 'static, // TODO: 'static :-/
    I: KadServerInterface + 'static,     // TODO: 'static :-/
{
    type Output = (
        KademliaServerController,
        Box<Future<Item = (), Error = IoError>>,
    );
    type Future = Box<Future<Item = Self::Output, Error = IoError>>;
    type NamesIter = iter::Once<(Bytes, ())>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        ConnectionUpgrade::<C>::protocol_names(&self.raw_proto)
    }

    #[inline]
    fn upgrade(self, incoming: C, id: (), endpoint: Endpoint, addr: &Multiaddr) -> Self::Future {
        let peer_id = {
            let mut iter = addr.iter();
            let protocol = iter.next();
            let after_proto = iter.next();
            match (protocol, after_proto) {
                (Some(AddrComponent::P2P(key)), None) | (Some(AddrComponent::IPFS(key)), None) => {
                    match PeerId::from_bytes(key) {
                        Ok(id) => id,
                        Err(_) => {
                            let err = IoError::new(
                                IoErrorKind::InvalidData,
                                "invalid peer ID sent by remote identification",
                            );
                            return Box::new(future::err(err));
                        }
                    }
                }
                _ => {
                    let err =
                        IoError::new(IoErrorKind::InvalidData, "couldn't identify connected node");
                    return Box::new(future::err(err));
                }
            }
        };

        let interface = self.interface;
        let future = self.raw_proto
            .upgrade(incoming, id, endpoint, addr)
            .map(move |connec| {
                let (tx, rx) = mpsc::unbounded();
                let future = kademlia_handler(connec, peer_id, rx, interface);
                let controller = KademliaServerController { inner: tx };
                (controller, future)
            });

        Box::new(future) as Box<_>
    }
}

/// Allows sending Kademlia requests and receiving responses.
#[derive(Debug, Clone)]
pub struct KademliaServerController {
    // In order to send a request, we use this sender to send a tuple. The first element of the
    // tuple is the message to send to the remote, and the second element is what is used to
    // receive the response. If the query doesn't expect a response (eg. `PUT_VALUE`), then the
    // one-shot sender will be dropped without being used.
    inner: mpsc::UnboundedSender<(KadMsg, oneshot::Sender<KadMsg>)>,
}

impl KademliaServerController {
    /// Sends a `FIND_NODE` query to the node and provides a future that will contain the response.
    // TODO: future item could be `impl Iterator` instead
    pub fn find_node(
        &self,
        searched_key: &PeerId,
    ) -> Box<Future<Item = Vec<Peer>, Error = IoError>> {
        let message = protocol::KadMsg::FindNodeReq {
            key: searched_key.clone().into_bytes(),
        };

        let (tx, rx) = oneshot::channel();

        match self.inner.unbounded_send((message, tx)) {
            Ok(()) => (),
            Err(_) => {
                let fut = future::err(IoError::new(
                    IoErrorKind::ConnectionAborted,
                    "connection to remote has aborted",
                ));
                return Box::new(fut) as Box<_>;
            }
        };

        let future = rx.map_err(|_| {
            IoError::new(
                IoErrorKind::ConnectionAborted,
                "connection to remote has aborted",
            )
        }).and_then(|msg| match msg {
            KadMsg::FindNodeRes { closer_peers, .. } => Ok(closer_peers),
            _ => Err(IoError::new(
                IoErrorKind::InvalidData,
                "invalid response type received from the remote",
            )),
        });

        Box::new(future) as Box<_>
    }

    /// Sends a `PING` query to the node. Because of the way the protocol is designed, there is
    /// no way to differentiate between a ping and a pong. Therefore this function doesn't return a
    /// future, and the only way to be notified of the result is through the `kbuckets_update`
    /// method in the `KadServerInterface` trait.
    pub fn ping(&self) -> Result<(), IoError> {
        // Dummy channel.
        let (tx, _rx) = oneshot::channel();
        match self.inner.unbounded_send((protocol::KadMsg::Ping, tx)) {
            Ok(()) => Ok(()),
            Err(_) => Err(IoError::new(
                IoErrorKind::ConnectionAborted,
                "connection to remote has aborted",
            )),
        }
    }
}

// Handles a newly-opened Kademlia stream with a remote peer.
//
// Takes a `Stream` and `Sink` of Kademlia messages representing the connection to the client,
// plus the ID of the peer that we are handling, plus a `Receiver` that will receive messages to
// transmit to that connection, plus the interface.
//
// Returns a `Future` that must be resolved in order for progress to work. It will never yield any
// item (unless both `rx` and `kad_bistream` are closed) but will propagate any I/O of protocol
// error that could happen. If the `Receiver` closes, no error is generated.
fn kademlia_handler<'a, S, I>(
    kad_bistream: S,
    peer_id: PeerId,
    rx: mpsc::UnboundedReceiver<(KadMsg, oneshot::Sender<KadMsg>)>,
    interface: I,
) -> Box<Future<Item = (), Error = IoError> + 'a>
where
    S: Stream<Item = KadMsg, Error = IoError> + Sink<SinkItem = KadMsg, SinkError = IoError> + 'a,
    I: KadServerInterface + Clone + 'a,
{
    let (kad_sink, kad_stream) = kad_bistream
        .sink_map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
        .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
        .split();

    // We combine `kad_stream` and `rx` into one so that the loop wakes up whenever either
    // generates something.
    let messages = rx.map(|(m, o)| (m, Some(o)))
        .map_err(|_| unreachable!())
        .select(kad_stream.map(|m| (m, None)));

    // Loop forever.
    let future = future::loop_fn(
        (kad_sink, messages, VecDeque::new(), 0),
        move |(kad_sink, messages, mut send_back_queue, mut expected_pongs)| {
            let interface = interface.clone();
            let peer_id = peer_id.clone();

            // The `send_back_queue` is a queue of `UnboundedSender`s in the correct order of
            // expected answers.
            // Whenever we send a message to the remote and this message expects a response, we
            // push the sender to the end of `send_back_queue`. Whenever a remote sends us a
            // response, we pop the first element of `send_back_queue`.

            // The value of `expected_pongs` is the number of PING requests that we sent and that
            // haven't been answered by the remote yet. Because of the way the protocol is designed,
            // there is no way to differentiate between a ping and a pong. Therefore whenever we
            // send a ping request we suppose that the next ping we receive is an answer, even
            // though that may not be the case in reality.
            // Because of this behaviour, pings do not pop from the `send_back_queue`.

            messages
                .into_future()
                .map_err(|(err, _)| err)
                .and_then(move |(message, rest)| {
                    if let Some((_, None)) = message {
                        // If we received a message from the remote (as opposed to a message from
                        // `rx`) then we update the k-buckets.
                        interface.kbuckets_update(&peer_id);
                    }

                    match message {
                        None => {
                            // Both the connection stream and `rx` are empty, so we break the loop.
                            let future = future::ok(future::Loop::Break(()));
                            Box::new(future) as Box<Future<Item = _, Error = _>>
                        }
                        Some((message @ KadMsg::PutValue { .. }, Some(_))) => {
                            // A `PutValue` message has been received on `rx`. Contrary to other
                            // types of messages, this one doesn't expect any answer and therefore
                            // we ignore the sender.
                            let future = kad_sink.send(message).map(move |kad_sink| {
                                future::Loop::Continue((
                                    kad_sink,
                                    rest,
                                    send_back_queue,
                                    expected_pongs,
                                ))
                            });
                            Box::new(future) as Box<_>
                        }
                        Some((message @ KadMsg::Ping { .. }, Some(_))) => {
                            // A `Ping` message has been received on `rx`.
                            expected_pongs += 1;
                            let future = kad_sink.send(message).map(move |kad_sink| {
                                future::Loop::Continue((
                                    kad_sink,
                                    rest,
                                    send_back_queue,
                                    expected_pongs,
                                ))
                            });
                            Box::new(future) as Box<_>
                        }
                        Some((message, Some(send_back))) => {
                            // Any message other than `PutValue` or `Ping` has been received on
                            // `rx`. Send it to the remote.
                            let future = kad_sink.send(message).map(move |kad_sink| {
                                send_back_queue.push_back(send_back);
                                future::Loop::Continue((
                                    kad_sink,
                                    rest,
                                    send_back_queue,
                                    expected_pongs,
                                ))
                            });
                            Box::new(future) as Box<_>
                        }
                        Some((KadMsg::Ping, None)) => {
                            // Note: The way the protocol was designed, there is no way to
                            //		 differentiate between a ping and a pong.
                            if expected_pongs == 0 {
                                let message = KadMsg::Ping;
                                let future = kad_sink.send(message).map(move |kad_sink| {
                                    future::Loop::Continue((
                                        kad_sink,
                                        rest,
                                        send_back_queue,
                                        expected_pongs,
                                    ))
                                });
                                Box::new(future) as Box<_>
                            } else {
                                expected_pongs -= 1;
                                let future = future::ok({
                                    future::Loop::Continue((
                                        kad_sink,
                                        rest,
                                        send_back_queue,
                                        expected_pongs,
                                    ))
                                });
                                Box::new(future) as Box<_>
                            }
                        }
                        Some((message @ KadMsg::FindNodeRes { .. }, None))
                        | Some((message @ KadMsg::GetValueRes { .. }, None)) => {
                            // `FindNodeRes` or `GetValueRes` received on the socket.
                            // Send it back through `send_back_queue`.
                            if let Some(send_back) = send_back_queue.pop_front() {
                                let _ = send_back.send(message);
                                let future = future::ok(future::Loop::Continue((
                                    kad_sink,
                                    rest,
                                    send_back_queue,
                                    expected_pongs,
                                )));
                                return Box::new(future) as Box<_>;
                            } else {
                                let future = future::err(IoErrorKind::InvalidData.into());
                                return Box::new(future) as Box<_>;
                            }
                        }
                        Some((KadMsg::FindNodeReq { key, .. }, None)) => {
                            // `FindNodeReq` received on the socket.
                            let message = handle_find_node_req(&interface, &key);
                            let future = kad_sink.send(message).map(move |kad_sink| {
                                future::Loop::Continue((
                                    kad_sink,
                                    rest,
                                    send_back_queue,
                                    expected_pongs,
                                ))
                            });
                            Box::new(future) as Box<_>
                        }
                        Some((KadMsg::GetValueReq { key, .. }, None)) => {
                            // `GetValueReq` received on the socket.
                            let message = handle_get_value_req(&interface, &key);
                            let future = kad_sink.send(message).map(move |kad_sink| {
                                future::Loop::Continue((
                                    kad_sink,
                                    rest,
                                    send_back_queue,
                                    expected_pongs,
                                ))
                            });
                            Box::new(future) as Box<_>
                        }
                        Some((KadMsg::PutValue { .. }, None)) => {
                            // `PutValue` received on the socket.
                            handle_put_value_req(&interface);
                            let future = future::ok({
                                future::Loop::Continue((
                                    kad_sink,
                                    rest,
                                    send_back_queue,
                                    expected_pongs,
                                ))
                            });
                            Box::new(future) as Box<_>
                        }
                    }
                })
        },
    );

    Box::new(future) as Box<Future<Item = (), Error = IoError>>
}

// Builds a `KadMsg` that handles a `FIND_NODE` request received from the remote.
fn handle_find_node_req<I>(interface: &I, requested_key: &[u8]) -> KadMsg
where
    I: ?Sized + KadServerInterface,
{
    let peer_id = match PeerId::from_bytes(requested_key.to_vec()) {
        // TODO: suboptimal
        Ok(id) => id,
        Err(_) => {
            return KadMsg::FindNodeRes {
                closer_peers: vec![],
            }
        }
    };

    let closer_peers = interface
        .kbuckets_find_closest(&peer_id)
        .into_iter()
        .map(|peer| {
            let (multiaddrs, connection_ty) = interface.peer_info(&peer);
            protocol::Peer {
                node_id: peer,
                multiaddrs: multiaddrs,
                connection_ty: connection_ty,
            }
        })
        .collect();

    KadMsg::FindNodeRes { closer_peers }
}

// Builds a `KadMsg` that handles a `FIND_VALUE` request received from the remote.
fn handle_get_value_req<I>(_interface: &I, _requested_key: &[u8]) -> KadMsg
where
    I: ?Sized + KadServerInterface,
{
    unimplemented!()
}

// Handles a `STORE` request received from the remote.
fn handle_put_value_req<I>(_interface: &I)
where
    I: ?Sized + KadServerInterface,
{
    unimplemented!()
}
