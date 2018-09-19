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

use fnv::FnvHashMap;
use futures::{prelude::*, sync::mpsc, sync::oneshot, task};
use muxing::StreamMuxer;
use nodes::node::Substream;
use nodes::handled_node::{HandledNode, NodeHandler};
use smallvec::SmallVec;
use std::collections::hash_map::{Entry, OccupiedEntry};
use std::io::Error as IoError;
use tokio_executor;
use void::Void;
use {Multiaddr, PeerId};

// TODO: make generic over PeerId

// Implementor notes
// =================
//
// This collection of nodes spawns a task for each individual node to process. This means that
// events happen on the background at the same time as the `CollectionStream` is being polled.
//
// In order to make the API non-racy and avoid issues, we totally separate the state in the
// `CollectionStream` and the states that the task nodes can access. They are only allowed to
// exchange messages. The state in the `CollectionStream` is therefore delayed compared to the
// tasks, and is updated only when `poll()` is called.
//
// The only thing that we must be careful about is substreams, as they are "detached" from the
// state of the `CollectionStream` and allowed to process in parallel. This is why there is no
// "substream closed" event being reported, as it could potentially create confusions and race
// conditions in the user's code. See similar comments in the documentation of `NodeStream`.

/// Implementation of `Stream` that handles a collection of nodes.
// TODO: implement Debug
pub struct CollectionStream<TInEvent, TOutEvent> {
    /// List of nodes, with a sender allowing to communicate messages.
    nodes: FnvHashMap<PeerId, (ReachAttemptId, mpsc::UnboundedSender<TInEvent>)>,
    /// Known state of a task. Tasks are identified by the reach attempt ID.
    tasks: FnvHashMap<ReachAttemptId, TaskKnownState>,
    /// Identifier for the next task to spawn.
    next_task_id: ReachAttemptId,

    /// List of node tasks to spawn.
    // TODO: stronger typing?
    to_spawn: SmallVec<[Box<Future<Item = (), Error = ()> + Send>; 8]>,
    /// Task to notify when an element is added to `to_spawn`.
    to_notify: Option<task::Task>,

    /// Sender to emit events to the outside. Meant to be cloned and sent to tasks.
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TInEvent, TOutEvent>, ReachAttemptId)>,
    /// Receiver side for the events.
    events_rx: mpsc::UnboundedReceiver<(InToExtMessage<TInEvent, TOutEvent>, ReachAttemptId)>,
}

/// State of a task, as known by the frontend (the `ColletionStream`). Asynchronous compared to
/// the actual state.
enum TaskKnownState {
    /// Task is attempting to reach a peer.
    Pending { interrupt: oneshot::Sender<()> },
    /// The user interrupted this task.
    Interrupted,
    /// The task is connected to a peer.
    Connected(PeerId),
}

impl TaskKnownState {
    /// Returns `true` for `Pending`.
    #[inline]
    fn is_pending(&self) -> bool {
        match *self {
            TaskKnownState::Pending { .. } => true,
            TaskKnownState::Interrupted => false,
            TaskKnownState::Connected(_) => false,
        }
    }
}

/// Event that can happen on the `CollectionStream`.
// TODO: implement Debug
pub enum CollectionEvent<TOutEvent> {
    /// A connection to a node has succeeded.
    NodeReached {
        /// Identifier of the node.
        peer_id: PeerId,
        /// Identifier of the reach attempt that succeeded.
        id: ReachAttemptId,
    },

    /// A connection to a node has succeeded and replaces a former connection.
    ///
    /// The opened substreams of the former node will keep working (unless the remote decides to
    /// close them).
    NodeReplaced {
        /// Identifier of the node.
        peer_id: PeerId,
        /// Identifier of the reach attempt that succeeded.
        id: ReachAttemptId,
    },

    /// A connection to a node has been closed.
    ///
    /// This happens once both the inbound and outbound channels are closed, and no more outbound
    /// substream attempt is pending.
    NodeClosed {
        /// Identifier of the node.
        peer_id: PeerId,
    },

    /// A connection to a node has errored.
    NodeError {
        /// Identifier of the node.
        peer_id: PeerId,
        /// The error that happened.
        error: IoError,
    },

    /// An error happened on the future that was trying to reach a node.
    ReachError {
        /// Identifier of the reach attempt that failed.
        id: ReachAttemptId,
        /// Error that happened on the future.
        error: IoError,
    },

    /// A node has produced an event.
    NodeEvent {
        /// Identifier of the node.
        peer_id: PeerId,
        /// The produced event.
        event: TOutEvent,
    },
}

/// Identifier for a future that attempts to reach a node.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReachAttemptId(usize);

impl<TInEvent, TOutEvent> CollectionStream<TInEvent, TOutEvent> {
    /// Creates a new empty collection.
    #[inline]
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::unbounded();

        CollectionStream {
            nodes: Default::default(),
            tasks: Default::default(),
            next_task_id: ReachAttemptId(0),
            to_spawn: SmallVec::new(),
            to_notify: None,
            events_tx,
            events_rx,
        }
    }

    /// Adds to the collection a future that tries to reach a remote.
    ///
    /// This method spawns a task dedicated to resolving this future and processing the node's
    /// events.
    pub fn add_reach_attempt<TFut, TMuxer, TAddrFut, THandler>(&mut self, future: TFut, handler: THandler)
        -> ReachAttemptId
    where
        TFut: Future<Item = ((PeerId, TMuxer), TAddrFut), Error = IoError> + Send + 'static,
        TAddrFut: Future<Item = Multiaddr, Error = IoError> + Send + 'static,
        THandler: NodeHandler<Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        THandler::OutboundOpenInfo: Send + 'static,     // TODO: shouldn't be required?
        TMuxer: StreamMuxer + Send + Sync + 'static,  // TODO: Send + Sync + 'static shouldn't be required
        TMuxer::OutboundSubstream: Send + 'static,  // TODO: shouldn't be required
    {
        let reach_attempt_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (interrupt_tx, interrupt_rx) = oneshot::channel();
        self.tasks.insert(
            reach_attempt_id,
            TaskKnownState::Pending {
                interrupt: interrupt_tx,
            },
        );

        let task = Box::new(NodeTask {
            inner: NodeTaskInner::Future {
                future,
                interrupt: interrupt_rx,
                handler: Some(handler),
            },
            events_tx: self.events_tx.clone(),
            id: reach_attempt_id,
        });

        self.to_spawn.push(task);

        if let Some(task) = self.to_notify.take() {
            task.notify();
        }

        reach_attempt_id
    }

    /// Interrupts a reach attempt.
    ///
    /// Returns `Ok` if something was interrupted, and `Err` if the ID is not or no longer valid.
    pub fn interrupt(&mut self, id: ReachAttemptId) -> Result<(), ()> {
        match self.tasks.entry(id) {
            Entry::Vacant(_) => return Err(()),
            Entry::Occupied(mut entry) => {
                match entry.get() {
                    &TaskKnownState::Connected(_) => return Err(()),
                    &TaskKnownState::Interrupted => return Err(()),
                    &TaskKnownState::Pending { .. } => (),
                };

                match entry.insert(TaskKnownState::Interrupted) {
                    TaskKnownState::Pending { interrupt } => {
                        let _ = interrupt.send(());
                    }
                    TaskKnownState::Interrupted | TaskKnownState::Connected(_) => unreachable!(),
                };
            }
        }

        Ok(())
    }

    /// Sends an event to all nodes.
    pub fn broadcast_event(&mut self, event: &TInEvent)
    where TInEvent: Clone,
    {
        for &(_, ref sender) in self.nodes.values() {
            let _ = sender.unbounded_send(event.clone()); // TODO: unwrap
        }
    }

    /// Grants access to an object that allows controlling a node of the collection.
    ///
    /// Returns `None` if we don't have a connection to this peer.
    #[inline]
    pub fn peer_mut(&mut self, id: &PeerId) -> Option<PeerMut<TInEvent>> {
        match self.nodes.entry(id.clone()) {
            Entry::Occupied(inner) => Some(PeerMut {
                inner,
                tasks: &mut self.tasks,
            }),
            Entry::Vacant(_) => None,
        }
    }

    /// Returns true if we are connected to the given peer.
    ///
    /// This will return true only after a `NodeReached` event has been produced by `poll()`.
    #[inline]
    pub fn has_connection(&self, id: &PeerId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Returns a list of all the active connections.
    ///
    /// Does not include reach attempts that haven't reached any target yet.
    #[inline]
    pub fn connections(&self) -> impl Iterator<Item = &PeerId> {
        self.nodes.keys()
    }
}

/// Access to a peer in the collection.
pub struct PeerMut<'a, TInEvent: 'a> {
    inner: OccupiedEntry<'a, PeerId, (ReachAttemptId, mpsc::UnboundedSender<TInEvent>)>,
    tasks: &'a mut FnvHashMap<ReachAttemptId, TaskKnownState>,
}

impl<'a, TInEvent> PeerMut<'a, TInEvent> {
    /// Sends an event to the given node.
    #[inline]
    pub fn send_event(&mut self, event: TInEvent) {
        // It is possible that the sender is closed if the task has already finished but we
        // haven't been polled in the meanwhile.
        let _ = self.inner.get_mut().1.unbounded_send(event);
    }

    /// Closes the connections to this node.
    ///
    /// No further event will be generated for this node.
    pub fn close(self) {
        let (peer_id, (task_id, _)) = self.inner.remove_entry();
        // Set the task to `Interrupted` so that we ignore further messages from this closed node.
        match self.tasks.insert(task_id, TaskKnownState::Interrupted) {
            Some(TaskKnownState::Connected(ref p)) if p == &peer_id => (),
            None
            | Some(TaskKnownState::Connected(_))
            | Some(TaskKnownState::Pending { .. })
            | Some(TaskKnownState::Interrupted) => {
                panic!("The task_id we have was retreived from self.nodes. We insert in this \
                       hashmap when we reach a node, in which case we also insert Connected in \
                       self.tasks with the corresponding peer ID. Once a task is Connected, it \
                       can no longer switch back to Pending. We switch the state to Interrupted \
                       only when we remove from self.nodes at the same time.")
            },
        }
    }
}

impl<TInEvent, TOutEvent> Stream for CollectionStream<TInEvent, TOutEvent> {
    type Item = CollectionEvent<TOutEvent>;
    type Error = Void; // TODO: use ! once stable

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        for to_spawn in self.to_spawn.drain() {
            tokio_executor::spawn(to_spawn);
        }

        loop {
            return match self.events_rx.poll() {
                Ok(Async::Ready(Some((InToExtMessage::NodeEvent(event), task_id)))) => {
                    let peer_id = match self.tasks.get(&task_id) {
                        Some(TaskKnownState::Connected(ref peer_id)) => peer_id.clone(),
                        Some(TaskKnownState::Interrupted) => continue, // Ignore messages from this task.
                        None | Some(TaskKnownState::Pending { .. }) => {
                            panic!("We insert Pending in self.tasks when a node is opened, and we \
                                    set it to Connected when we receive a NodeReached event from \
                                    this task. The way the task works, we are guaranteed to \
                                    a NodeReached event before any NodeEvent.")
                        }
                    };

                    Ok(Async::Ready(Some(CollectionEvent::NodeEvent {
                        peer_id,
                        event,
                    })))
                }
                Ok(Async::Ready(Some((InToExtMessage::NodeReached(peer_id, sender), task_id)))) => {
                    {
                        let existing = match self.tasks.get_mut(&task_id) {
                            Some(state) => state,
                            None => panic!("We insert in self.tasks the corresponding task_id \
                                            when we create a task, and we only remove from it \
                                            if we receive the events NodeClosed, NodeError and \
                                            ReachError, that are produced only when a task ends. \
                                            After a task ends, we don't receive any more event \
                                            from it.")
                        };

                        match existing {
                            TaskKnownState::Pending { .. } => (),
                            TaskKnownState::Interrupted => continue,
                            TaskKnownState::Connected(_) => {
                                panic!("The only code that sets a task to the Connected state \
                                        is when we receive a NodeReached event. If we are already \
                                        connected, that would mean we received NodeReached twice, \
                                        which is not possible as a task changes state after \
                                        sending this event.")
                            },
                        }

                        *existing = TaskKnownState::Connected(peer_id.clone());
                    }

                    let replaced_node = self.nodes.insert(peer_id.clone(), (task_id, sender));
                    if let Some(replaced_node) = replaced_node {
                        let old = self.tasks.insert(replaced_node.0, TaskKnownState::Interrupted);
                        debug_assert_eq!(old.map(|s| s.is_pending()), Some(false));
                        Ok(Async::Ready(Some(CollectionEvent::NodeReplaced {
                            peer_id,
                            id: task_id,
                        })))
                    } else {
                        Ok(Async::Ready(Some(CollectionEvent::NodeReached {
                            peer_id,
                            id: task_id,
                        })))
                    }
                }
                Ok(Async::Ready(Some((InToExtMessage::NodeClosed, task_id)))) => {
                    let peer_id = match self.tasks.remove(&task_id) {
                        Some(TaskKnownState::Connected(peer_id)) => peer_id.clone(),
                        Some(TaskKnownState::Interrupted) => continue, // Ignore messages from this task.
                        None | Some(TaskKnownState::Pending { .. }) => {
                            panic!("We insert Pending in self.tasks when a node is opened, and we \
                                    set it to Connected when we receive a NodeReached event from \
                                    this task. The way the task works, we are guaranteed to \
                                    a NodeReached event before a NodeClosed.")
                        }
                    };

                    let val = self.nodes.remove(&peer_id);
                    debug_assert!(val.is_some());
                    Ok(Async::Ready(Some(CollectionEvent::NodeClosed { peer_id })))
                }
                Ok(Async::Ready(Some((InToExtMessage::NodeError(err), task_id)))) => {
                    let peer_id = match self.tasks.remove(&task_id) {
                        Some(TaskKnownState::Connected(peer_id)) => peer_id.clone(),
                        Some(TaskKnownState::Interrupted) => continue, // Ignore messages from this task.
                        None | Some(TaskKnownState::Pending { .. }) => {
                            panic!("We insert Pending in self.tasks when a node is opened, and we \
                                    set it to Connected when we receive a NodeReached event from \
                                    this task. The way the task works, we are guaranteed to \
                                    a NodeReached event before a NodeError.")
                        }
                    };

                    let val = self.nodes.remove(&peer_id);
                    debug_assert!(val.is_some());
                    Ok(Async::Ready(Some(CollectionEvent::NodeError {
                        peer_id,
                        error: err,
                    })))
                }
                Ok(Async::Ready(Some((InToExtMessage::ReachError(err), task_id)))) => {
                    match self.tasks.remove(&task_id) {
                        Some(TaskKnownState::Interrupted) => continue,
                        Some(TaskKnownState::Pending { .. }) => (),
                        None | Some(TaskKnownState::Connected(_)) => {
                            panic!("We insert Pending in self.tasks when a node is opened, and we \
                                    set it to Connected when we receive a NodeReached event from \
                                    this task. The way the task works, we are guaranteed to \
                                    a NodeReached event before a ReachError.")
                        }
                    };

                    Ok(Async::Ready(Some(CollectionEvent::ReachError {
                        id: task_id,
                        error: err,
                    })))
                }
                Ok(Async::NotReady) => {
                    self.to_notify = Some(task::current());
                    Ok(Async::NotReady)
                }
                Ok(Async::Ready(None)) => {
                    unreachable!("The sender is in self as well, therefore the receiver never \
                                  closes.")
                },
                Err(()) => unreachable!("An unbounded receiver never errors"),
            };
        }
    }
}

/// Message to transmit from a task to the public API.
enum InToExtMessage<TInEvent, TOutEvent> {
    /// A connection to a node has succeeded.
    /// Closing the returned sender will end the task.
    NodeReached(PeerId, mpsc::UnboundedSender<TInEvent>),
    NodeClosed,
    NodeError(IoError),
    ReachError(IoError),
    /// An event from the node.
    NodeEvent(TOutEvent),
}

/// Implementation of `Future` that handles a single node, and all the communications between
/// the various components of the `CollectionStream`.
struct NodeTask<TFut, TMuxer, TAddrFut, THandler, TInEvent, TOutEvent>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
{
    /// Sender to transmit events to the outside.
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TInEvent, TOutEvent>, ReachAttemptId)>,
    /// Inner state of the `NodeTask`.
    inner: NodeTaskInner<TFut, TMuxer, TAddrFut, THandler, TInEvent>,
    /// Identifier of the attempt.
    id: ReachAttemptId,
}

enum NodeTaskInner<TFut, TMuxer, TAddrFut, THandler, TInEvent>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
{
    /// Future to resolve to connect to the node.
    Future {
        /// The future that will attempt to reach the node.
        future: TFut,
        /// Allows interrupting the attempt.
        interrupt: oneshot::Receiver<()>,
        /// The handler that will be used to build the `HandledNode`.
        handler: Option<THandler>,
    },

    /// Fully functional node.
    Node {
        /// The object that is actually processing things.
        node: HandledNode<TMuxer, TAddrFut, THandler>,
        /// Receiving end for events sent from the main `CollectionStream`. `None` if closed.
        in_events_rx: Option<mpsc::UnboundedReceiver<TInEvent>>,
    },
}

impl<TFut, TMuxer, TAddrFut, THandler, TInEvent, TOutEvent> Future for
    NodeTask<TFut, TMuxer, TAddrFut, THandler, TInEvent, TOutEvent>
where
    TMuxer: StreamMuxer,
    TFut: Future<Item = ((PeerId, TMuxer), TAddrFut), Error = IoError>,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
    THandler: NodeHandler<Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent>,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        // Remember that this poll function is dedicated to a single node and is run
        // asynchronously.

        // First, handle if we are still trying to reach a node.
        let new_state = if let NodeTaskInner::Future {
            ref mut future,
            ref mut interrupt,
            ref mut handler,
        } = self.inner
        {
            match interrupt.poll() {
                Ok(Async::NotReady) => (),
                Ok(Async::Ready(())) | Err(_) => return Ok(Async::Ready(())),
            }

            match future.poll() {
                Ok(Async::Ready(((peer_id, muxer), addr_fut))) => {
                    let (sender, rx) = mpsc::unbounded();
                    let event = InToExtMessage::NodeReached(peer_id, sender);
                    let _ = self.events_tx.unbounded_send((event, self.id));

                    let handler = handler.take()
                        .expect("The handler is only extracted right before we switch state");

                    Some(NodeTaskInner::Node {
                        node: HandledNode::new(muxer, addr_fut, handler),
                        in_events_rx: Some(rx),
                    })
                }
                Ok(Async::NotReady) => {
                    return Ok(Async::NotReady);
                }
                Err(error) => {
                    // End the task
                    let event = InToExtMessage::ReachError(error);
                    let _ = self.events_tx.unbounded_send((event, self.id));
                    return Ok(Async::Ready(()));
                }
            }
        } else {
            None
        };

        if let Some(new_state) = new_state {
            self.inner = new_state;
        }

        // Then handle if we're a node.
        if let NodeTaskInner::Node {
            ref mut node,
            ref mut in_events_rx,
        } = self.inner
        {
            // Start by handling commands received from the outside of the task.
            if let Some(mut local_rx) = in_events_rx.take() {
                *in_events_rx = loop {
                    match local_rx.poll() {
                        Ok(Async::Ready(Some(event))) => {
                            node.inject_event(event);
                        },
                        Ok(Async::Ready(None)) => {
                            // Node closed by the external API ; start shutdown process.
                            node.shutdown();
                            break None;
                        }
                        Ok(Async::NotReady) => break Some(local_rx),
                        Err(()) => unreachable!("An unbounded receiver never errors"),
                    }
                };
            }

            // Process the node.
            loop {
                match node.poll() {
                    Ok(Async::NotReady) => break,
                    Ok(Async::Ready(Some(event))) => {
                        let event = InToExtMessage::NodeEvent(event);
                        let _ = self.events_tx.unbounded_send((event, self.id));
                    }
                    Ok(Async::Ready(None)) => {
                        let event = InToExtMessage::NodeClosed;
                        let _ = self.events_tx.unbounded_send((event, self.id));
                        return Ok(Async::Ready(())); // End the task.
                    }
                    Err(err) => {
                        let event = InToExtMessage::NodeError(err);
                        let _ = self.events_tx.unbounded_send((event, self.id));
                        return Ok(Async::Ready(())); // End the task.
                    }
                }
            }
        }

        // Nothing's ready. The current task should have been registered by all of the inner
        // handlers.
        Ok(Async::NotReady)
    }
}
