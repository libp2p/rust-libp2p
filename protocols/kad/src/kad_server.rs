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
//! - Create a `KadConnecConfig` object. This struct implements `ConnectionUpgrade`.
//!
//! - Update a connection through that `KadConnecConfig`. The output yields you a
//!   `KadConnecController` and a stream that must be driven to completion. The controller
//!   allows you to perform queries and receive responses. The stream produces incoming requests
//!   from the remote.
//!
//! This `KadConnecController` is usually extracted and stored in some sort of hash map in an
//! `Arc` in order to be available whenever we need to request something from a node.

use bytes::Bytes;
use futures::sync::{mpsc, oneshot};
use futures::{future, Future, Sink, stream, Stream};
use libp2p_core::{PeerId, upgrade::{InboundUpgrade, UpgradeInfo}};
use log::{debug, warn};
use multihash::Multihash;
use protocol::{self, KadMsg, KademliaProtocolConfig, KadPeer};
use std::collections::VecDeque;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};

/// Configuration for a Kademlia server.
///
/// Implements `ConnectionUpgrade`. On a successful upgrade, produces a `KadConnecController`
/// and a `Future`. The controller lets you send queries to the remote and receive answers, while
/// the `Future` must be driven to completion in order for things to work.
#[derive(Debug, Clone)]
pub struct KadConnecConfig {
    raw_proto: KademliaProtocolConfig,
}

impl KadConnecConfig {
    /// Builds a configuration object for an upcoming Kademlia server.
    #[inline]
    pub fn new() -> Self {
        KadConnecConfig {
            raw_proto: KademliaProtocolConfig,
        }
    }
}

impl UpgradeInfo for KadConnecConfig {
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;
    type UpgradeId = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        self.raw_proto.protocol_names()
    }
}

impl<C> InboundUpgrade<C> for KadConnecConfig
where
    C: AsyncRead + AsyncWrite + Send + 'static, // TODO: 'static :-/
{
    type Output = (
        KadConnecController,
        Box<Stream<Item = KadIncomingRequest, Error = IoError> + Send>,
    );
    type Error = IoError;
    type Future = future::Map<<KademliaProtocolConfig as InboundUpgrade<C>>::Future, fn(<KademliaProtocolConfig as InboundUpgrade<C>>::Output) -> Self::Output>;

    #[inline]
    fn upgrade_inbound(self, incoming: C, id: Self::UpgradeId) -> Self::Future {
        self.raw_proto
            .upgrade_inbound(incoming, id)
            .map(build_from_sink_stream)
    }
}

/// Allows sending Kademlia requests and receiving responses.
#[derive(Debug, Clone)]
pub struct KadConnecController {
    // In order to send a request, we use this sender to send a tuple. The first element of the
    // tuple is the message to send to the remote, and the second element is what is used to
    // receive the response. If the query doesn't expect a response (e.g. `PUT_VALUE`), then the
    // one-shot sender will be dropped without being used.
    inner: mpsc::UnboundedSender<(KadMsg, oneshot::Sender<KadMsg>)>,
}

impl KadConnecController {
    /// Sends a `FIND_NODE` query to the node and provides a future that will contain the response.
    // TODO: future item could be `impl Iterator` instead
    pub fn find_node(
        &self,
        searched_key: &PeerId,
    ) -> impl Future<Item = Vec<KadPeer>, Error = IoError> {
        let message = protocol::KadMsg::FindNodeReq {
            key: searched_key.clone().into(),
        };

        let (tx, rx) = oneshot::channel();

        match self.inner.unbounded_send((message, tx)) {
            Ok(()) => (),
            Err(_) => {
                let fut = future::err(IoError::new(
                    IoErrorKind::ConnectionAborted,
                    "connection to remote has aborted",
                ));

                return future::Either::B(fut);
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

        future::Either::A(future)
    }

    /// Sends a `GET_PROVIDERS` query to the node and provides a future that will contain the response.
    // TODO: future item could be `impl Iterator` instead
    pub fn get_providers(
        &self,
        searched_key: &Multihash,
    ) -> impl Future<Item = (Vec<KadPeer>, Vec<KadPeer>), Error = IoError> {
        let message = protocol::KadMsg::GetProvidersReq {
            key: searched_key.clone(),
        };

        let (tx, rx) = oneshot::channel();

        match self.inner.unbounded_send((message, tx)) {
            Ok(()) => (),
            Err(_) => {
                let fut = future::err(IoError::new(
                    IoErrorKind::ConnectionAborted,
                    "connection to remote has aborted",
                ));

                return future::Either::B(fut);
            }
        };

        let future = rx.map_err(|_| {
            IoError::new(
                IoErrorKind::ConnectionAborted,
                "connection to remote has aborted",
            )
        }).and_then(|msg| match msg {
            KadMsg::GetProvidersRes { closer_peers, provider_peers } => Ok((closer_peers, provider_peers)),
            _ => Err(IoError::new(
                IoErrorKind::InvalidData,
                "invalid response type received from the remote",
            )),
        });

        future::Either::A(future)
    }

    /// Sends an `ADD_PROVIDER` message to the node.
    pub fn add_provider(&self, key: Multihash, provider_peer: KadPeer) -> Result<(), IoError> {
        // Dummy channel, as the `tx` is going to be dropped anyway.
        let (tx, _rx) = oneshot::channel();
        let message = protocol::KadMsg::AddProvider {
            key,
            provider_peer,
        };
        match self.inner.unbounded_send((message, tx)) {
            Ok(()) => Ok(()),
            Err(_) => Err(IoError::new(
                IoErrorKind::ConnectionAborted,
                "connection to remote has aborted",
            )),
        }
    }

    /// Sends a `PING` query to the node. Because of the way the protocol is designed, there is
    /// no way to differentiate between a ping and a pong. Therefore this function doesn't return a
    /// future, and the only way to be notified of the result is through the stream.
    pub fn ping(&self) -> Result<(), IoError> {
        // Dummy channel, as the `tx` is going to be dropped anyway.
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

/// Request received from the remote.
pub enum KadIncomingRequest {
    /// Find the nodes closest to `searched`.
    FindNode {
        /// The value being searched.
        searched: PeerId,
        /// Object to use to respond to the request.
        responder: KadFindNodeRespond,
    },

    /// Find the nodes closest to `searched` and return the known providers for `searched`.
    GetProviders {
        /// The value being searched.
        searched: Multihash,
        /// Object to use to respond to the request.
        responder: KadGetProvidersRespond,
    },

    /// Registers a provider for the given key.
    ///
    /// The local node is supposed to remember this and return the provider on a `GetProviders`
    /// request for the given key.
    AddProvider {
        /// The key of the provider.
        key: Multihash,
        /// The provider to register.
        provider_peer: KadPeer,
    },

    /// Received either a ping or a pong.
    PingPong,

    // TODO: PutValue and FindValue
}

/// Object used to respond to `FindNode` queries from remotes.
pub struct KadFindNodeRespond {
    inner: oneshot::Sender<KadMsg>,
}

impl KadFindNodeRespond {
    /// Respond to the `FindNode` request.
    pub fn respond<I>(self, peers: I)
        where I: IntoIterator<Item = protocol::KadPeer>
    {
        let _ = self.inner.send(KadMsg::FindNodeRes {
            closer_peers: peers.into_iter().collect()
        });
    }
}

/// Object used to respond to `GetProviders` queries from remotes.
pub struct KadGetProvidersRespond {
    inner: oneshot::Sender<KadMsg>,
}

impl KadGetProvidersRespond {
    /// Respond to the `GetProviders` request.
    pub fn respond<Ic, Ip>(self, closest_peers: Ic, providers: Ip)
        where Ic: IntoIterator<Item = protocol::KadPeer>,
              Ip: IntoIterator<Item = protocol::KadPeer>,
    {
        let _ = self.inner.send(KadMsg::GetProvidersRes {
            closer_peers: closest_peers.into_iter().collect(),
            provider_peers: providers.into_iter().collect(),
        });
    }
}

// Builds a controller and stream from a stream/sink of raw messages.
fn build_from_sink_stream<'a, S>(connec: S) -> (KadConnecController, Box<Stream<Item = KadIncomingRequest, Error = IoError> + Send + 'a>)
where S: Sink<SinkItem = KadMsg, SinkError = IoError> + Stream<Item = KadMsg, Error = IoError> + Send + 'a
{
    let (tx, rx) = mpsc::unbounded();
    let future = kademlia_handler(connec, rx);
    let controller = KadConnecController { inner: tx };
    (controller, future)
}

// Handles a newly-opened Kademlia stream with a remote peer.
//
// Takes a `Stream` and `Sink` of Kademlia messages representing the connection to the client,
// plus a `Receiver` that will receive messages to transmit to that connection.
//
// Returns a `Stream` that must be resolved in order for progress to work. The `Stream` will
// produce objects that represent the requests sent by the remote. These requests must be answered
// immediately before the stream continues to produce items.
fn kademlia_handler<'a, S>(
    kad_bistream: S,
    rq_rx: mpsc::UnboundedReceiver<(KadMsg, oneshot::Sender<KadMsg>)>,
) -> Box<Stream<Item = KadIncomingRequest, Error = IoError> + Send + 'a>
where
    S: Stream<Item = KadMsg, Error = IoError> + Sink<SinkItem = KadMsg, SinkError = IoError> + Send + 'a,
{
    let (kad_sink, kad_stream) = kad_bistream.split();

    // This is a stream of futures containing local responses.
    // Every time we receive a request from the remote, we create a `oneshot::channel()` and send
    // the receiving end to `responders_tx`.
    // This way, if a future is available on `responders_rx`, we block until it produces the
    // response.
    let (responders_tx, responders_rx) = mpsc::unbounded();

    // We combine all the streams into one so that the loop wakes up whenever any generates
    // something.
    enum EventSource {
        Remote(KadMsg),
        LocalRequest(KadMsg, oneshot::Sender<KadMsg>),
        LocalResponse(oneshot::Receiver<KadMsg>),
        Finished,
    }

    let events = {
        let responders = responders_rx
            .map(|m| EventSource::LocalResponse(m))
            .map_err(|_| unreachable!());
        let rq_rx = rq_rx
            .map(|(m, o)| EventSource::LocalRequest(m, o))
            .map_err(|_| unreachable!());
        let kad_stream = kad_stream
            .map(|m| EventSource::Remote(m))
            .chain(future::ok(EventSource::Finished).into_stream());
        responders.select(rq_rx).select(kad_stream)
    };

    let stream = stream::unfold((events, kad_sink, responders_tx, VecDeque::new(), 0u32, false),
        move |(events, kad_sink, responders_tx, mut send_back_queue, expected_pongs, finished)| {
            if finished {
                return None;
            }

            Some(events
                .into_future()
                .map_err(|(err, _)| err)
                .and_then(move |(message, events)| -> Box<Future<Item = _, Error = _> + Send> {
                    match message {
                        Some(EventSource::Finished) | None => {
                            let future = future::ok({
                                let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, true);
                                (None, state)
                            });
                            Box::new(future)
                        },
                        Some(EventSource::LocalResponse(message)) => {
                            let future = message
                                .map_err(|err| {
                                    // The user destroyed the responder without responding.
                                    warn!("Kad responder object destroyed without responding");
                                    // TODO: what to do here? we have to close the connection
                                    IoError::new(IoErrorKind::Other, err)
                                })
                                .and_then(move |message| {
                                    kad_sink
                                        .send(message)
                                        .map(move |kad_sink| {
                                            let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                            (None, state)
                                        })
                                });
                            Box::new(future)
                        },
                        Some(EventSource::LocalRequest(message @ KadMsg::PutValue { .. }, _)) |
                        Some(EventSource::LocalRequest(message @ KadMsg::AddProvider { .. }, _)) => {
                            // A `PutValue` or `AddProvider` request. Contrary to other types of
                            // messages, these ones don't expect any answer and therefore we ignore
                            // the sender.
                            let future = kad_sink
                                .send(message)
                                .map(move |kad_sink| {
                                    let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                    (None, state)
                                });
                            Box::new(future) as Box<_>
                        }
                        Some(EventSource::LocalRequest(message @ KadMsg::Ping { .. }, _)) => {
                            // A local `Ping` request.
                            let expected_pongs = expected_pongs.checked_add(1)
                                .expect("overflow in number of simultaneous pings");
                            let future = kad_sink
                                .send(message)
                                .map(move |kad_sink| {
                                    let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                    (None, state)
                                });
                            Box::new(future) as Box<_>
                        }
                        Some(EventSource::LocalRequest(message, send_back)) => {
                            // Any local request other than `PutValue` or `Ping`.
                            send_back_queue.push_back(send_back);
                            let future = kad_sink
                                .send(message)
                                .map(move |kad_sink| {
                                    let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                    (None, state)
                                });
                            Box::new(future) as Box<_>
                        }
                        Some(EventSource::Remote(KadMsg::Ping)) => {
                            // The way the protocol was designed, there is no way to differentiate
                            // between a ping and a pong.
                            if let Some(expected_pongs) = expected_pongs.checked_sub(1) {
                                // Maybe we received a PONG, or maybe we received a PONG, no way
                                // to tell. If it was a PING and we expected a PONG, then the
                                // remote will see its PING answered only when it PONGs us.
                                let future = future::ok({
                                    let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                    let rq = KadIncomingRequest::PingPong;
                                    (Some(rq), state)
                                });
                                Box::new(future) as Box<_>
                            } else {
                                let future = kad_sink
                                    .send(KadMsg::Ping)
                                    .map(move |kad_sink| {
                                        let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                        let rq = KadIncomingRequest::PingPong;
                                        (Some(rq), state)
                                    });
                                Box::new(future) as Box<_>
                            }
                        }
                        Some(EventSource::Remote(message @ KadMsg::FindNodeRes { .. }))
                        | Some(EventSource::Remote(message @ KadMsg::GetValueRes { .. }))
                        | Some(EventSource::Remote(message @ KadMsg::GetProvidersRes { .. })) => {
                            // `FindNodeRes`, `GetValueRes` or `GetProvidersRes` received on the socket.
                            // Send it back through `send_back_queue`.
                            if let Some(send_back) = send_back_queue.pop_front() {
                                let _ = send_back.send(message);
                                let future = future::ok({
                                    let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                    (None, state)
                                });
                                Box::new(future)
                            } else {
                                debug!("Remote sent a Kad response but we didn't request anything");
                                let future = future::err(IoErrorKind::InvalidData.into());
                                Box::new(future)
                            }
                        }
                        Some(EventSource::Remote(KadMsg::FindNodeReq { key })) => {
                            let (tx, rx) = oneshot::channel();
                            let _ = responders_tx.unbounded_send(rx);
                            let future = future::ok({
                                let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                let rq = KadIncomingRequest::FindNode {
                                    searched: key,
                                    responder: KadFindNodeRespond {
                                        inner: tx
                                    }
                                };
                                (Some(rq), state)
                            });

                            Box::new(future)
                        }
                        Some(EventSource::Remote(KadMsg::GetProvidersReq { key })) => {
                            let (tx, rx) = oneshot::channel();
                            let _ = responders_tx.unbounded_send(rx);
                            let future = future::ok({
                                let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                let rq = KadIncomingRequest::GetProviders {
                                    searched: key,
                                    responder: KadGetProvidersRespond {
                                        inner: tx
                                    }
                                };
                                (Some(rq), state)
                            });

                            Box::new(future)
                        }
                        Some(EventSource::Remote(KadMsg::AddProvider { key, provider_peer })) => {
                            let future = future::ok({
                                let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                                let rq = KadIncomingRequest::AddProvider { key, provider_peer };
                                (Some(rq), state)
                            });
                            Box::new(future) as Box<_>
                        }
                        Some(EventSource::Remote(KadMsg::GetValueReq { .. })) => {
                            warn!("GET_VALUE requests are not implemented yet");
                            let future = future::err(IoError::new(IoErrorKind::Other,
                                "GET_VALUE requests are not implemented yet"));
                            return Box::new(future);
                        }
                        Some(EventSource::Remote(KadMsg::PutValue { .. })) => {
                            warn!("PUT_VALUE requests are not implemented yet");
                            let state = (events, kad_sink, responders_tx, send_back_queue, expected_pongs, finished);
                            let future = future::ok((None, state));
                            return Box::new(future);
                        }
                    }
                }))
    }).filter_map(|val| val);

    Box::new(stream) as Box<Stream<Item = _, Error = IoError> + Send>
}

#[cfg(test)]
mod tests {
    use std::io::Error as IoError;
    use std::iter;
    use futures::{Future, Poll, Sink, StartSend, Stream};
    use futures::sync::mpsc;
    use kad_server::{self, KadIncomingRequest, KadConnecController};
    use libp2p_core::PublicKey;
    use protocol::{KadConnectionType, KadPeer};
    use rand;

    // This struct merges a stream and a sink and is quite useful for tests.
    struct Wrapper<St, Si>(St, Si);
    impl<St, Si> Stream for Wrapper<St, Si>
    where
        St: Stream,
    {
        type Item = St::Item;
        type Error = St::Error;
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            self.0.poll()
        }
    }
    impl<St, Si> Sink for Wrapper<St, Si>
    where
        Si: Sink,
    {
        type SinkItem = Si::SinkItem;
        type SinkError = Si::SinkError;
        fn start_send(
            &mut self,
            item: Self::SinkItem,
        ) -> StartSend<Self::SinkItem, Self::SinkError> {
            self.1.start_send(item)
        }
        fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
            self.1.poll_complete()
        }
        fn close(&mut self) -> Poll<(), Self::SinkError> {
            self.1.close()
        }
    }

    fn build_test() -> (KadConnecController, impl Stream<Item = KadIncomingRequest, Error = IoError>, KadConnecController, impl Stream<Item = KadIncomingRequest, Error = IoError>) {
        let (a_to_b, b_from_a) = mpsc::unbounded();
        let (b_to_a, a_from_b) = mpsc::unbounded();

        let sink_stream_a = Wrapper(a_from_b, a_to_b)
            .map_err(|_| panic!()).sink_map_err(|_| panic!());
        let sink_stream_b = Wrapper(b_from_a, b_to_a)
            .map_err(|_| panic!()).sink_map_err(|_| panic!());

        let (controller_a, stream_events_a) = kad_server::build_from_sink_stream(sink_stream_a);
        let (controller_b, stream_events_b) = kad_server::build_from_sink_stream(sink_stream_b);
        (controller_a, stream_events_a, controller_b, stream_events_b)
    }

    #[test]
    fn ping_response() {
        let (controller_a, stream_events_a, _controller_b, stream_events_b) = build_test();

        controller_a.ping().unwrap();

        let streams = stream_events_a.map(|ev| (ev, "a"))
            .select(stream_events_b.map(|ev| (ev, "b")));
        match streams.into_future().map_err(|(err, _)| err).wait().unwrap() {
            (Some((KadIncomingRequest::PingPong, "b")), _) => {},
            _ => panic!()
        }
    }

    #[test]
    fn find_node_response() {
        let (controller_a, stream_events_a, _controller_b, stream_events_b) = build_test();

        let random_peer_id = {
            let buf = (0 .. 1024).map(|_| -> u8 { rand::random() }).collect::<Vec<_>>();
            PublicKey::Rsa(buf).into_peer_id()
        };

        let find_node_fut = controller_a.find_node(&random_peer_id);

        let example_response = KadPeer {
            node_id: {
                let buf = (0 .. 1024).map(|_| -> u8 { rand::random() }).collect::<Vec<_>>();
                PublicKey::Rsa(buf).into_peer_id()
            },
            multiaddrs: Vec::new(),
            connection_ty: KadConnectionType::Connected,
        };

        let streams = stream_events_a.map(|ev| (ev, "a"))
            .select(stream_events_b.map(|ev| (ev, "b")));

        let streams = match streams.into_future().map_err(|(err, _)| err).wait().unwrap() {
            (Some((KadIncomingRequest::FindNode { searched, responder }, "b")), streams) => {
                assert_eq!(searched, random_peer_id);
                responder.respond(iter::once(example_response.clone()));
                streams
            },
            _ => panic!()
        };

        let resp = streams.into_future().map_err(|(err, _)| err).map(|_| unreachable!())
            .select(find_node_fut)
            .map_err(|_| -> IoError { panic!() });
        assert_eq!(resp.wait().unwrap().0, vec![example_response]);
    }
}
