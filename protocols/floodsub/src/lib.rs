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

extern crate bs58;
extern crate byteorder;
extern crate bytes;
extern crate fnv;
extern crate futures;
extern crate libp2p_core;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate parking_lot;
extern crate protobuf;
extern crate smallvec;
extern crate tokio_codec;
extern crate tokio_io;
extern crate unsigned_varint;

mod rpc_proto;
mod topic;

pub use self::topic::{Topic, TopicBuilder, TopicHash};

use byteorder::{BigEndian, WriteBytesExt};
use bytes::{Bytes, BytesMut};
use fnv::{FnvHashMap, FnvHashSet, FnvHasher};
use futures::sync::mpsc;
use futures::{future, Future, Poll, Sink, Stream};
use libp2p_core::{ConnectionUpgrade, Endpoint, PeerId};
use log::Level;
use multiaddr::{AddrComponent, Multiaddr};
use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};
use protobuf::Message as ProtobufMessage;
use smallvec::SmallVec;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

/// Implementation of the `ConnectionUpgrade` for the floodsub protocol.
#[derive(Debug, Clone)]
pub struct FloodSubUpgrade {
    inner: Arc<Inner>,
}

impl FloodSubUpgrade {
    /// Builds a new `FloodSubUpgrade`. Also returns a `FloodSubReceiver` that will stream incoming
    /// messages for the floodsub system.
    pub fn new(my_id: PeerId) -> (FloodSubUpgrade, FloodSubReceiver) {
        let (output_tx, output_rx) = mpsc::unbounded();

        let inner = Arc::new(Inner {
            peer_id: my_id.into_bytes(),
            output_tx: output_tx,
            remote_connections: RwLock::new(FnvHashMap::default()),
            subscribed_topics: RwLock::new(Vec::new()),
            seq_no: AtomicUsize::new(0),
            received: Mutex::new(FnvHashSet::default()),
        });

        let upgrade = FloodSubUpgrade { inner: inner };

        let receiver = FloodSubReceiver { inner: output_rx };

        (upgrade, receiver)
    }
}

impl<C, Maf> ConnectionUpgrade<C, Maf> for FloodSubUpgrade
where
    C: AsyncRead + AsyncWrite + 'static,
    Maf: Future<Item = Multiaddr, Error = IoError> + 'static,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once(("/floodsub/1.0.0".into(), ()))
    }

    type Output = FloodSubFuture;
    type MultiaddrFuture = future::FutureResult<Multiaddr, IoError>;
    type Future = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>>;

    #[inline]
    fn upgrade(
        self,
        socket: C,
        _: Self::UpgradeIdentifier,
        _: Endpoint,
        remote_addr: Maf,
    ) -> Self::Future {
        debug!("Upgrading connection as floodsub");

        let future = remote_addr.and_then(move |remote_addr| {
            // Whenever a new node connects, we send to it a message containing the topics we are
            // already subscribed to.
            let init_msg: Vec<u8> = {
                let subscribed_topics = self.inner.subscribed_topics.read();
                let mut proto = rpc_proto::RPC::new();

                for topic in subscribed_topics.iter() {
                    let mut subscription = rpc_proto::RPC_SubOpts::new();
                    subscription.set_subscribe(true);
                    subscription.set_topicid(topic.hash().clone().into_string());
                    proto.mut_subscriptions().push(subscription);
                }

                proto
                    .write_to_bytes()
                    .expect("programmer error: the protobuf message should always be valid")
            };

            // Split the socket into writing and reading parts.
            let (floodsub_sink, floodsub_stream) = Framed::new(socket, codec::UviBytes::default())
                .sink_map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
                .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
                .split();

            // Build the channel that will be used to communicate outgoing message to this remote.
            let (input_tx, input_rx) = mpsc::unbounded();
            input_tx
                .unbounded_send(init_msg.into())
                .expect("newly-created channel should always be open");
            self.inner.remote_connections.write().insert(
                remote_addr.clone(),
                RemoteInfo {
                    sender: input_tx,
                    subscribed_topics: RwLock::new(FnvHashSet::default()),
                },
            );

            // Combine the socket read and the outgoing messages input, so that we can wake up when
            // either happens.
            let messages = input_rx
                .map(|m| (m, MessageSource::FromChannel))
                .map_err(|_| unreachable!("channel streams should never produce an error"))
                .select(floodsub_stream.map(|m| (m, MessageSource::FromSocket)));

            #[derive(Debug)]
            enum MessageSource {
                FromSocket,
                FromChannel,
            }

            let inner = self.inner.clone();
            let remote_addr_ret = future::ok(remote_addr.clone());
            let future = future::loop_fn(
                (floodsub_sink, messages),
                move |(floodsub_sink, messages)| {
                    let inner = inner.clone();
                    let remote_addr = remote_addr.clone();

                    messages
                        .into_future()
                        .map_err(|(err, _)| err)
                        .and_then(move |(input, rest)| {
                            match input {
                                Some((bytes, MessageSource::FromSocket)) => {
                                    // Received a packet from the remote.
                                    let fut = match handle_packet_received(bytes, inner, &remote_addr) {
                                        Ok(()) => {
                                            future::ok(future::Loop::Continue((floodsub_sink, rest)))
                                        }
                                        Err(err) => future::err(err),
                                    };
                                    Box::new(fut) as Box<_>
                                }

                                Some((bytes, MessageSource::FromChannel)) => {
                                    // Received a packet from the channel.
                                    // Need to send a message to remote.
                                    trace!("Effectively sending message to remote");
                                    let future = floodsub_sink.send(bytes).map(|floodsub_sink| {
                                        future::Loop::Continue((floodsub_sink, rest))
                                    });
                                    Box::new(future) as Box<_>
                                }

                                None => {
                                    // Both the connection stream and `rx` are empty, so we break
                                    // the loop.
                                    trace!("Pubsub future clean finish");
                                    // TODO: what if multiple connections?
                                    inner.remote_connections.write().remove(&remote_addr);
                                    let future = future::ok(future::Loop::Break(()));
                                    Box::new(future) as Box<Future<Item = _, Error = _>>
                                }
                            }
                        })
                },
            );

            future::ok((FloodSubFuture {
                inner: Box::new(future) as Box<_>,
            }, remote_addr_ret))
        });

        Box::new(future) as Box<_>
    }
}

/// Allows one to control the behaviour of the floodsub system.
#[derive(Clone)]
pub struct FloodSubController {
    inner: Arc<Inner>,
}

struct Inner {
    // Our local peer ID multihash, to pass as the source.
    peer_id: Vec<u8>,

    // Channel where to send the messages that should be dispatched to the user.
    output_tx: mpsc::UnboundedSender<Message>,

    // Active connections with a remote.
    remote_connections: RwLock<FnvHashMap<Multiaddr, RemoteInfo>>,

    // List of topics we're subscribed to. Necessary in order to filter out messages that we
    // erroneously receive.
    subscribed_topics: RwLock<Vec<Topic>>,

    // Sequence number for the messages we send.
    seq_no: AtomicUsize,

    // We keep track of the messages we received (in the format `(remote ID, seq_no)`) so that we
    // don't dispatch the same message twice if we receive it twice on the network.
    // TODO: the `HashSet` will keep growing indefinitely :-/
    received: Mutex<FnvHashSet<u64>>,
}

struct RemoteInfo {
    // Sender to send data over the socket to that host.
    sender: mpsc::UnboundedSender<BytesMut>,
    // Topics the remote is registered to.
    subscribed_topics: RwLock<FnvHashSet<TopicHash>>,
}

impl fmt::Debug for Inner {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Inner")
            .field("peer_id", &self.peer_id)
            .field(
                "num_remote_connections",
                &self.remote_connections.read().len(),
            )
            .field("subscribed_topics", &*self.subscribed_topics.read())
            .field("seq_no", &self.seq_no)
            .field("received", &self.received)
            .finish()
    }
}

impl FloodSubController {
    /// Builds a new controller for floodsub.
    #[inline]
    pub fn new(upgrade: &FloodSubUpgrade) -> Self {
        FloodSubController {
            inner: upgrade.inner.clone(),
        }
    }

    /// Subscribe to a topic. When a node on the network sends a message for that topic, we will
    /// likely receive it.
    ///
    /// It is not guaranteed that we receive every single message published on the network.
    #[inline]
    pub fn subscribe(&self, topic: &Topic) {
        // This function exists for convenience.
        self.subscribe_many(iter::once(topic));
    }

    /// Same as `subscribe`, but subscribes to multiple topics at once.
    ///
    /// Since this results in a single packet sent to the remotes, it is preferable to use this
    /// method when subscribing to multiple topics at once rather than call `subscribe` multiple
    /// times.
    #[inline]
    pub fn subscribe_many<'a, I>(&self, topics: I)
    where
        I: IntoIterator<Item = &'a Topic>,
        I::IntoIter: Clone,
    {
        // This function exists for convenience.
        self.sub_unsub_multi(topics.into_iter().map::<_, fn(_) -> _>(|t| (t, true)))
    }

    /// Unsubscribe from a topic. We will no longer receive any message for this topic.
    ///
    /// If a message was sent to us before we are able to notify that we don't want messages
    /// anymore, then the message will be filtered out locally.
    #[inline]
    pub fn unsubscribe(&self, topic: &Topic) {
        // This function exists for convenience.
        self.unsubscribe_many(iter::once(topic));
    }

    /// Same as `unsubscribe` but unsubscribes from multiple topics at once.
    ///
    /// Since this results in a single packet sent to the remotes, it is preferable to use this
    /// method when ybsubscribing from multiple topics at once rather than call `unsubscribe`
    /// multiple times.
    #[inline]
    pub fn unsubscribe_many<'a, I>(&self, topics: I)
    where
        I: IntoIterator<Item = &'a Topic>,
        I::IntoIter: Clone,
    {
        // This function exists for convenience.
        self.sub_unsub_multi(topics.into_iter().map::<_, fn(_) -> _>(|t| (t, false)));
    }

    // Inner implementation. The iterator should produce a boolean that is true if we subscribe and
    // false if we unsubscribe.
    fn sub_unsub_multi<'a, I>(&self, topics: I)
    where
        I: IntoIterator<Item = (&'a Topic, bool)>,
        I::IntoIter: Clone,
    {
        let mut proto = rpc_proto::RPC::new();

        let topics = topics.into_iter();

        if log_enabled!(Level::Debug) {
            debug!("Queuing sub/unsub message ; sub = {:?} ; unsub = {:?}",
                topics.clone().filter(|t| t.1)
                        .map(|t| t.0.hash().clone().into_string())
                        .collect::<Vec<_>>(),
                topics.clone().filter(|t| !t.1)
                        .map(|t| t.0.hash().clone().into_string())
                        .collect::<Vec<_>>());
        }

        let mut subscribed_topics = self.inner.subscribed_topics.write();
        for (topic, subscribe) in topics {
            let mut subscription = rpc_proto::RPC_SubOpts::new();
            subscription.set_subscribe(subscribe);
            subscription.set_topicid(topic.hash().clone().into_string());
            proto.mut_subscriptions().push(subscription);

            if subscribe {
                subscribed_topics.push(topic.clone());
            } else {
                subscribed_topics.retain(|t| t.hash() != topic.hash())
            }
        }

        self.broadcast(proto, |_| true);
    }

    /// Publishes a message on the network for the specified topic
    #[inline]
    pub fn publish(&self, topic: &Topic, data: Vec<u8>) {
        // This function exists for convenience.
        self.publish_many(iter::once(topic), data)
    }

    /// Publishes a message on the network for the specified topics.
    ///
    /// Since this results in a single packet sent to the remotes, it is preferable to use this
    /// method when publishing multiple messages at once rather than call `publish` multiple
    /// times.
    pub fn publish_many<'a, I>(&self, topics: I, data: Vec<u8>)
    where
        I: IntoIterator<Item = &'a Topic>,
    {
        let topics = topics.into_iter().collect::<Vec<_>>();

        debug!("Queueing publish message ; topics = {:?} ; data_len = {:?}",
               topics.iter().map(|t| t.hash().clone().into_string()).collect::<Vec<_>>(),
               data.len());

        // Build the `Vec<u8>` containing our sequence number for this message.
        let seq_no_bytes = {
            let mut seqno_bytes = Vec::new();
            let seqn = self.inner.seq_no.fetch_add(1, Ordering::Relaxed);
            seqno_bytes
                .write_u64::<BigEndian>(seqn as u64)
                .expect("writing to a Vec never fails");
            seqno_bytes
        };

        // TODO: should handle encryption/authentication of the message

        let mut msg = rpc_proto::Message::new();
        msg.set_data(data);
        msg.set_from(self.inner.peer_id.clone());
        msg.set_seqno(seq_no_bytes.clone());
        msg.set_topicIDs(
            topics
                .iter()
                .map(|t| t.hash().clone().into_string())
                .collect(),
        );

        let mut proto = rpc_proto::RPC::new();
        proto.mut_publish().push(msg);

        // Insert into `received` so that we ignore the message if a remote sends it back to us.
        self.inner
            .received
            .lock()
            .insert(hash((self.inner.peer_id.clone(), seq_no_bytes)));

        self.broadcast(proto, |r_top| {
            topics.iter().any(|t| r_top.iter().any(|to| to == t.hash()))
        });
    }

    // Internal function that dispatches an `RPC` protobuf struct to all the connected remotes
    // for which `filter` returns true.
    fn broadcast<F>(&self, message: rpc_proto::RPC, mut filter: F)
    where
        F: FnMut(&FnvHashSet<TopicHash>) -> bool,
    {
        let bytes = message
            .write_to_bytes()
            .expect("protobuf message is always valid");

        let remote_connections = self.inner.remote_connections.upgradable_read();

        // Number of remotes we dispatched to, for logging purposes.
        let mut num_dispatched = 0;
        // Will store the addresses of remotes which we failed to send a message to and which
        // must be removed from the active connections.
        // We use a smallvec of 6 elements because it is unlikely that we lost connection to more
        // than 6 elements at once.
        let mut failed_to_send: SmallVec<[_; 6]> = SmallVec::new();
        for (remote_addr, remote) in remote_connections.iter() {
            if !filter(&remote.subscribed_topics.read()) {
                continue;
            }

            num_dispatched += 1;
            match remote.sender.unbounded_send(bytes.clone().into()) {
                Ok(_) => (),
                Err(_) => {
                    trace!("Failed to dispatch message to {} because channel was closed",
                           remote_addr);
                    failed_to_send.push(remote_addr.clone());
                }
            }
        }

        // Remove the remotes which we failed to send a message to.
        if !failed_to_send.is_empty() {
            // If we fail to upgrade the read lock to a write lock, just ignore `failed_to_send`.
            if let Ok(mut remote_connections) = RwLockUpgradableReadGuard::try_upgrade(remote_connections) {
                for failed_to_send in failed_to_send {
                    remote_connections.remove(&failed_to_send);
                }
            }
        }

        debug!("Message queued for {} remotes", num_dispatched);
    }
}

/// Implementation of `Stream` that provides messages for the subscribed topics you subscribed to.
pub struct FloodSubReceiver {
    inner: mpsc::UnboundedReceiver<Message>,
}

impl Stream for FloodSubReceiver {
    type Item = Message;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner
            .poll()
            .map_err(|_| unreachable!("UnboundedReceiver cannot err"))
    }
}

impl fmt::Debug for FloodSubReceiver {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("FloodSubReceiver").finish()
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
#[must_use = "futures do nothing unless polled"]
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

impl fmt::Debug for FloodSubFuture {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("FloodSubFuture").finish()
    }
}

// Handles when a packet is received on a connection.
//
// - `bytes` contains the raw data.
// - `remote_addr` is the address of the sender.
fn handle_packet_received(
    bytes: BytesMut,
    inner: Arc<Inner>,
    remote_addr: &Multiaddr,
) -> Result<(), IoError> {
    trace!("Received packet from {}", remote_addr);

    // Parsing attempt.
    let mut input = match protobuf::parse_from_bytes::<rpc_proto::RPC>(&bytes) {
        Ok(msg) => msg,
        Err(err) => {
            debug!("Failed to parse protobuf message ; err = {:?}", err);
            return Err(err.into());
        }
    };

    // Update the topics the remote is subscribed to.
    if !input.get_subscriptions().is_empty() {
        let remote_connec = inner.remote_connections.write();
        // TODO: what if multiple entries?
        let remote = &remote_connec[remote_addr];
        let mut topics = remote.subscribed_topics.write();
        for subscription in input.mut_subscriptions().iter_mut() {
            let topic = TopicHash::from_raw(subscription.take_topicid());
            let subscribe = subscription.get_subscribe();
            if subscribe {
                trace!("Remote {} subscribed to {:?}", remote_addr, topic); topics.insert(topic);
            } else {
                trace!("Remote {} unsubscribed from {:?}", remote_addr, topic);
                topics.remove(&topic);
            }
        }
    }

    // Handle the messages coming from the remote.
    for publish in input.mut_publish().iter_mut() {
        let from = publish.take_from();
        // We maintain a list of the messages that have already been
        // processed so that we don't process the same message twice.
        // Each message is identified by the `(from, seqno)` tuple.
        if !inner
            .received
            .lock()
            .insert(hash((from.clone(), publish.take_seqno())))
        {
            trace!("Skipping message because we had already received it ; payload = {} bytes",
                   publish.get_data().len());
            continue;
        }

        let peer_id = match PeerId::from_bytes(from.clone()) {
            Ok(id) => id,
            Err(err) => {
                trace!("Parsing PeerId failed: {:?}. Skipping.", err);
                continue
            }
        };

        let from: Multiaddr = AddrComponent::P2P(peer_id.into()).into();

        let topics = publish
            .take_topicIDs()
            .into_iter()
            .map(|h| TopicHash::from_raw(h))
            .collect::<Vec<_>>();

        trace!("Processing message for topics {:?} ; payload = {} bytes",
               topics,
               publish.get_data().len());

        // TODO: should check encryption/authentication of the message

        // Broadcast the message to all the other remotes.
        {
            let remote_connections = inner.remote_connections.read();
            for (addr, info) in remote_connections.iter() {
                let st = info.subscribed_topics.read();
                if !topics.iter().any(|t| st.contains(t)) {
                    continue;
                }
                // TODO: don't send back to the remote that just sent it
                trace!("Broadcasting received message to {}", addr);
                let _ = info.sender.unbounded_send(bytes.clone());
            }
        }

        // Send the message locally if relevant.
        let dispatch_locally = {
            let subscribed_topics = inner.subscribed_topics.read();
            topics
                .iter()
                .any(|t| subscribed_topics.iter().any(|topic| topic.hash() == t))
        };
        if dispatch_locally {
            // Ignore if channel is closed.
            trace!("Dispatching message locally");
            let _ = inner.output_tx.unbounded_send(Message {
                source: from,
                data: publish.take_data(),
                topics: topics,
            });
        } else {
            trace!("Message not dispatched locally as we are not subscribed to any of the topics");
        }
    }

    Ok(())
}

// Shortcut function that hashes a value.
#[inline]
fn hash<V: Hash>(value: V) -> u64 {
    let mut h = FnvHasher::default();
    value.hash(&mut h);
    h.finish()
}
