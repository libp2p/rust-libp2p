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

extern crate bytes;
extern crate byteorder;
extern crate futures;
extern crate libp2p_swarm;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate parking_lot;
extern crate protobuf;
extern crate tokio_io;
extern crate varint;

mod rpc_proto;
mod topic;

pub use self::topic::{TopicBuilder, TopicHash};

use std::collections::{HashMap, HashSet};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use bytes::{Bytes, BytesMut};
use byteorder::{BigEndian, WriteBytesExt};
use futures::{future, Future, Sink, Stream, Poll};
use futures::sync::mpsc;
use libp2p_swarm::{ConnectionUpgrade, Endpoint, SwarmController, MuxedTransport};
use multiaddr::{Multiaddr, AddrComponent};
use parking_lot::{Mutex, RwLock};
use protobuf::Message as ProtobufMessage;
use tokio_io::{AsyncRead, AsyncWrite};
use varint::VarintCodec;

/// Implementation of the `ConnectionUpgrade` for the floodsub protocol.
pub struct FloodSubUpgrade {
    inner: Arc<Inner>,
}

impl FloodSubUpgrade {
    /// Builds a new `FloodSubUpgrade`. Also returns a `FloodSubReceiver` that will stream incoming
    /// messages for the floodsub system.
    pub fn new() -> (FloodSubUpgrade, FloodSubReceiver) {
        let (output_tx, output_rx) = mpsc::unbounded();

        let inner = Arc::new(Inner {
            output_tx: output_tx,
            remote_connections: RwLock::new(HashMap::new()),
            subscribed_topics: RwLock::new(HashSet::new()),
            seq_no: AtomicUsize::new(0),
            received: Mutex::new(HashSet::new()),
        });

        let upgrade = FloodSubUpgrade {
            inner: inner,
        };

        let receiver = FloodSubReceiver {
            inner: output_rx,
        };

        (upgrade, receiver)
    }
}

impl<C> ConnectionUpgrade<C> for FloodSubUpgrade
    where C: AsyncRead + AsyncWrite + 'static
{
	type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
	type UpgradeIdentifier = ();

	#[inline]
	fn protocol_names(&self) -> Self::NamesIter {
		iter::once(("/floodsub/1.0.0".into(), ()))
	}

	type Output = FloodSubFuture;
	type Future = future::FutureResult<Self::Output, IoError>;

	#[inline]
	fn upgrade(self, socket: C, _: Self::UpgradeIdentifier, _: Endpoint, remote_addr: &Multiaddr)
			   -> Self::Future
	{
        let socket = socket.framed(VarintCodec::default());

        let (floodsub_sink, floodsub_stream) = socket
    		.sink_map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
    		.map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
    		.split();

        let (input_tx, input_rx) = mpsc::unbounded();
        self.inner.remote_connections.write().insert(remote_addr.clone(), RemoteInfo {
            sender: input_tx,
            subscribed_topics: RwLock::new(HashSet::new()),
        });

	    let messages = input_rx.map(|m| (m, true))
		    .map_err(|_| unreachable!())
		    .select(floodsub_stream.map(|m| (m, false)));

        let inner = self.inner.clone();
        let remote_addr = remote_addr.clone();

        let future = future::loop_fn((floodsub_sink, messages), move |(floodsub_sink, messages)| {
            let inner = inner.clone();
            let remote_addr = remote_addr.clone();

            messages
                .into_future()
                .map_err(|(err, _)| err)
                .and_then(move |(input, rest)| {
                    match input {
                        Some((bytes, false)) => {
                            let mut input = match protobuf::parse_from_bytes::<rpc_proto::RPC>(&bytes) {
                                Ok(msg) => msg,
                                Err(err) => {
                                    debug!(target: "libp2p-floodsub", "Failed to parse protobuf \
                                                                     message ; err = {:?}", err);
                                    let future = future::err(err.into());
                                    return Box::new(future) as Box<_>;
                                },
                            };

                            // TODO: handle `received` to avoid duplicate messages

                            if !input.get_publish().is_empty() {
                                let mut remote_connec = inner.remote_connections.write();
                                let mut remote = remote_connec.get(&remote_addr).unwrap();       // TODO: what if multiple entries?
                                let mut topics = remote.subscribed_topics.write();
                                for subscription in input.mut_subscriptions().iter_mut() {
                                    let topic = TopicHash::from_raw(subscription.take_topicid());
                                    let subscribe = subscription.get_subscribe();
                                    if subscribe {
                                        topics.insert(topic);
                                    } else {
                                        topics.remove(&topic);
                                    }
                                }
                            }

                            for publish in input.mut_publish().iter_mut() {
                                let topics = publish
                                    .take_topicIDs()
                                    .into_iter()
                                    .map(|h| TopicHash::from_raw(h))
                                    .collect::<Vec<_>>();

                                let subscribed_topics = inner.subscribed_topics.read();
                                let dispatch_locally = topics
                                    .iter()
                                    .any(|t| subscribed_topics.contains(t));

                                //let seqno = publish.take_seqno();
                                let out_msg = Message {
                                    source: AddrComponent::IPFS(publish.take_from()).into(),
                                    data: publish.take_data(),
                                    topics: topics,
                                };

                                if dispatch_locally {
                                    // Ignore if channel is closed.
                                    let _ = inner.output_tx.unbounded_send(out_msg);
                                }
                            }

                            let fut = future::ok(future::Loop::Continue((floodsub_sink, rest)));
                            Box::new(fut) as Box<_>
                        },
                        Some((bytes, true)) => {
                            // Sending message to remote.
                            trace!(target: "libp2p-floodsub", "Effectively sending message \
                                                             to remote");
                            let future = floodsub_sink
                                .send(bytes)
                                .map(|floodsub_sink| future::Loop::Continue((floodsub_sink, rest)));
                            Box::new(future) as Box<_>
                        },
                        None => {
                            // Both the connection stream and `rx` are empty, so we break the loop.
                            trace!(target: "libp2p-floodsub", "Pubsub future clean finish");
							let future = future::ok(future::Loop::Break(()));
							Box::new(future) as Box<Future<Item = _, Error = _>>
                        },
                    }
                })
        });

        future::ok(FloodSubFuture {
            inner: Box::new(future) as Box<_>
        })
	}
}

/// Allows one to control the behaviour of the floodsub system.
pub struct FloodSubController<T, C>
    where T: MuxedTransport + 'static,      // TODO: 'static :-/
          C: ConnectionUpgrade<T::RawConn> + 'static,      // TODO: 'static :-/
{
    inner: Arc<Inner>,
    swarm: SwarmController<T, C>,
}

struct Inner {
    // Channel where to send the messages that should be dispatched to the user.
    output_tx: mpsc::UnboundedSender<Message>,

    // Active connections with a remote.
    remote_connections: RwLock<HashMap<Multiaddr, RemoteInfo>>,

    // List of topics we're subscribed to. Necessary in order to filter out messages that we
    // erroneously receive.
    subscribed_topics: RwLock<HashSet<TopicHash>>,

    // Sequence number for the messages we send.
    seq_no: AtomicUsize,

    // We keep track of the messages we received (in the format `(remote, seq_no)`) so that we
    // don't dispatch the same message twice if we receive it twice on the network.
    received: Mutex<HashSet<(Multiaddr, Vec<u8>)>>,
}

struct RemoteInfo {
    // Sender to send data over the socket to that host.
    sender: mpsc::UnboundedSender<BytesMut>,
    // Topics the remote is registered to.
    subscribed_topics: RwLock<HashSet<TopicHash>>,
}

impl<T, C> FloodSubController<T, C>
    where T: MuxedTransport + 'static,      // TODO: 'static :-/
          C: ConnectionUpgrade<T::RawConn> + 'static,      // TODO: 'static :-/
{
    /// Builds a new controller for floodsub.
    pub fn new(upgrade: &FloodSubUpgrade, swarm: SwarmController<T, C>) -> FloodSubController<T, C> {
        FloodSubController {
            inner: upgrade.inner.clone(),
            swarm: swarm,
        }
    }

    /// Subscribe to a topic. When a node on the network sends a message for that topic, we will
    /// likely receive it.
    ///
    /// It is not guaranteed that we receive every single message published on the network.
    #[inline]
    pub fn subscribe(&self, topic: TopicHash) {
        self.subscribe_multi(iter::once(topic));
    }

    /// Same as `subscribe`, but subscribes to multiple topics at once.
    #[inline]
    pub fn subscribe_multi<I>(&self, topics: I)
        where I: IntoIterator<Item = TopicHash>,
              I::IntoIter: Clone,
    {
        self.sub_unsub_multi(topics.into_iter().map::<_, fn(_) -> _>(|t| (t, true)))
    }

    /// Unsubscribe from a topic. We will no longer receive any message for this topic.
    ///
    /// If a message was sent to us before we are able to notify that we don't want messages
    /// anymore, then the message will be filtered out locally.
    #[inline]
    pub fn unsubscribe(&self, topic: &TopicHash) {
        self.unsubscribe_multi(iter::once(topic));
    }

    /// Same as `unsubscribe` but unsubscribes from multiple topics at once.
    #[inline]
    pub fn unsubscribe_multi<'a, I>(&self, topics: I)
        where I: IntoIterator<Item = &'a TopicHash>,
              I::IntoIter: Clone,
    {
        self.sub_unsub_multi(topics.into_iter().map::<_, fn(_) -> _>(|t| (t.clone(), false)));
    }

    // Inner implementation. The iterator also produces a boolean that is true if we subscribe and
    // false if we unsubscribe.
    fn sub_unsub_multi<I>(&self, topics: I)
        where I: IntoIterator<Item = (TopicHash, bool)>,
              I::IntoIter: Clone,
    {
        let topics = topics.into_iter();

        let mut subscribed_topics = self.inner.subscribed_topics.write();

        let mut proto = rpc_proto::RPC::new();

        debug!(target: "libp2p-floodsub", "Queuing sub/unsub message ; sub = {:?} ; unsub = {:?}",
               topics.clone().filter(|t| t.1).map(|t| t.0.into_string()).collect::<Vec<_>>(),
               topics.clone().filter(|t| !t.1).map(|t| t.0.into_string()).collect::<Vec<_>>());

        for (topic, subscribe) in topics.clone() {
            let mut subscription = rpc_proto::RPC_SubOpts::new();
            subscription.set_subscribe(subscribe);
            subscription.set_topicid(topic.clone().into_string());
            proto.mut_subscriptions().push(subscription);

            subscribed_topics.insert(topic);
        }

        self.broadcast(proto, |_| true);
    }

    /// Publishes a message on the network for the specified topic.
    pub fn publish<'a, I>(&self, topics: I, data: Vec<u8>)
        where I: IntoIterator<Item = &'a TopicHash>,
              I::IntoIter: Clone,
    {
        let topics = topics.into_iter();

        debug!(target: "libp2p-floodsub", "Queueing publish message ; topics = {:?} ; data_len = {:?}",
               topics.clone().map(|t| t.clone().into_string()).collect::<Vec<_>>(), data.len());

        let mut msg = rpc_proto::Message::new();
        msg.set_data(data);
        //msg.set_from();

        let mut seqno_bytes = Vec::new();
        seqno_bytes.write_u64::<BigEndian>(self.inner.seq_no.fetch_add(1, Ordering::Relaxed) as u64)
            .expect("writing to a Vec never fails");
        msg.set_seqno(seqno_bytes);

        msg.set_topicIDs(topics.clone().map(|t| t.clone().into_string()).collect());

        let mut proto = rpc_proto::RPC::new();
        proto.mut_publish().push(msg);

        self.broadcast(proto, |r_top| topics.clone().any(|t| r_top.contains(t)));
    }

    // Internal function that dispatches an `RPC` protobuf struct to all the connected remotes
    // for which `filter` returns true.
    fn broadcast<F>(&self, message: rpc_proto::RPC, mut filter: F)
        where F: FnMut(&HashSet<TopicHash>) -> bool
    {
        let bytes = message.write_to_bytes().expect("protobuf message is always valid");
        let remote_connections = self.inner.remote_connections.read();

        let mut num_dispatched = 0;
        for remote in remote_connections.values() {
            if !filter(&remote.subscribed_topics.read()) {
                continue;
            }

            num_dispatched += 1;
            remote.sender.unbounded_send(bytes.clone().into());
        }

        debug!(target: "libp2p-floodsub", "Message queued for {} remotes", num_dispatched);
    }
}

/// Implementation of `Stream` that provides messages for the subscribed topics you subscribed to.
// TODO: implement Debug
pub struct FloodSubReceiver {
    inner: mpsc::UnboundedReceiver<Message>,
}

impl Stream for FloodSubReceiver {
    type Item = Message;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll().map_err(|_| unreachable!("UnboundedReceiver cannot err"))
    }
}

/// A message received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Message {
    /// Remote that sent the message.
    pub source: Multiaddr,

    /// Content of the message. Its meaning is out of scope of this library.
    pub data: Vec<u8>,

    /// List of topics of this message.
    ///
    /// Each message can belong to multiple topics at once.
    pub topics: Vec<TopicHash>,
}

/// Implementation of `Future` that must be driven to completion in order for floodsub to work.
// TODO: implement Debug
pub struct FloodSubFuture {
    inner: Box<Future<Item = (), Error = IoError>>,
}

impl Future for FloodSubFuture {
    type Item = ();
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll()
    }
}
