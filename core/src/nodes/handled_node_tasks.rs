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
use futures::{prelude::*, stream, sync::mpsc};
use muxing::StreamMuxer;
use nodes::node::Substream;
use nodes::handled_node::{HandledNode, NodeHandler};
use smallvec::SmallVec;
use std::collections::hash_map::{Entry, OccupiedEntry};
use std::io::Error as IoError;
use std::{fmt, mem};
use tokio_executor;
use void::Void;
use PeerId;

// TODO: make generic over PeerId

// Implementor notes
// =================
//
// This collection of nodes spawns a task for each individual node to process. This means that
// events happen on the background at the same time as the `HandledNodesTasks` is being polled.
//
// In order to make the API non-racy and avoid issues, we completely separate the state in the
// `HandledNodesTasks` from the states that the task nodes can access. They are only allowed to
// exchange messages. The state in the `HandledNodesTasks` is therefore delayed compared to the
// tasks, and is updated only when `poll()` is called.
//
// The only thing that we must be careful about is substreams, as they are "detached" from the
// state of the `HandledNodesTasks` and allowed to process in parallel. This is why there is no
// "substream closed" event being reported, as it could potentially create confusions and race
// conditions in the user's code. See similar comments in the documentation of `NodeStream`.

/// Implementation of `Stream` that handles a collection of nodes.
// TODO: implement Debug
pub struct HandledNodesTasks<TInEvent, TOutEvent, THandler> {
    /// For each active task, a sender allowing to transmit messages. Closing the sender interrupts
    /// the task. It is possible that we receive messages from tasks that used to be in this list
    /// but no longer are, in which case we should ignore them.
    tasks: FnvHashMap<TaskId, mpsc::UnboundedSender<TInEvent>>,

    /// Identifier for the next task to spawn.
    next_task_id: TaskId,

    /// List of node tasks to spawn.
    // TODO: stronger typing?
    to_spawn: SmallVec<[Box<Future<Item = (), Error = ()> + Send>; 8]>,

    /// Sender to emit events to the outside. Meant to be cloned and sent to tasks.
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TOutEvent, THandler>, TaskId)>,
    /// Receiver side for the events.
    events_rx: mpsc::UnboundedReceiver<(InToExtMessage<TOutEvent, THandler>, TaskId)>,
}

impl<TInEvent, TOutEvent, THandler> fmt::Debug for HandledNodesTasks<TInEvent, TOutEvent, THandler> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("HandledNodesTask").finish().expect("works");
        f.debug_list()
            .entries(self.tasks.keys().cloned())
            .finish()
    }
}

/// Event that can happen on the `HandledNodesTasks`.
#[derive(Debug)]
pub enum HandledNodesEvent<TOutEvent, THandler> {
    /// A task has been closed.
    ///
    /// This happens once the node handler closes or an error happens.
    // TODO: send back undelivered events?
    TaskClosed {
        /// Identifier of the task that closed.
        id: TaskId,
        /// What happened.
        result: Result<(), IoError>,
        /// If the task closed before reaching the node, this contains the handler that was passed
        /// to `add_reach_attempt`.
        handler: Option<THandler>,
    },

    /// A task has succeesfully connected to a node.
    NodeReached {
        /// Identifier of the task that succeeded.
        id: TaskId,
        /// Identifier of the node.
        peer_id: PeerId,
    },

    /// A task has produced an event.
    NodeEvent {
        /// Identifier of the task that produced the event.
        id: TaskId,
        /// The produced event.
        event: TOutEvent,
    },
}

/// Identifier for a future that attempts to reach a node.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(usize);

impl<TInEvent, TOutEvent, THandler> HandledNodesTasks<TInEvent, TOutEvent, THandler> {
    /// Creates a new empty collection.
    #[inline]
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::unbounded();

        HandledNodesTasks {
            tasks: Default::default(),
            next_task_id: TaskId(0),
            to_spawn: SmallVec::new(),
            events_tx,
            events_rx,
        }
    }

    /// Adds to the collection a future that tries to reach a node.
    ///
    /// This method spawns a task dedicated to resolving this future and processing the node's
    /// events.
    pub fn add_reach_attempt<TFut, TMuxer>(&mut self, future: TFut, handler: THandler)
        -> TaskId
    where
        TFut: Future<Item = (PeerId, TMuxer), Error = IoError> + Send + 'static,
        THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent> + Send + 'static,
        TInEvent: Send + 'static,
        TOutEvent: Send + 'static,
        THandler::OutboundOpenInfo: Send + 'static,     // TODO: shouldn't be required?
        TMuxer: StreamMuxer + Send + Sync + 'static,  // TODO: Send + Sync + 'static shouldn't be required
        TMuxer::OutboundSubstream: Send + 'static,  // TODO: shouldn't be required
    {
        let task_id = self.next_task_id;
        self.next_task_id.0 += 1;

        let (tx, rx) = mpsc::unbounded();
        self.tasks.insert(task_id, tx);

        let task = Box::new(NodeTask {
            inner: NodeTaskInner::Future {
                future,
                handler,
                events_buffer: Vec::new(),
            },
            events_tx: self.events_tx.clone(),
            in_events_rx: rx.fuse(),
            id: task_id,
        });

        self.to_spawn.push(task);
        println!("[HandledNodesTasks, add_reach_attempt] to_spawn len={}, task_id={:?}", self.to_spawn.len(), task_id);
        task_id
    }

    /// Sends an event to all the tasks, including the pending ones.
    pub fn broadcast_event(&mut self, event: &TInEvent)
    where TInEvent: Clone,
    {
        println!("[HandledNodesTasks, broadcast_event] sending {} events", self.tasks.len());
        for sender in self.tasks.values() {
            // Note: it is possible that sending an event fails if the background task has already
            // finished, but the local state hasn't reflected that yet because it hasn't been
            // polled. This is not an error situation.
            println!("[HandledNodesTasks, broadcast_event]  sending event");
            let _ = sender.unbounded_send(event.clone());
        }
    }

    /// Grants access to an object that allows controlling a task of the collection.
    ///
    /// Returns `None` if the task id is invalid.
    #[inline]
    pub fn task(&mut self, id: TaskId) -> Option<Task<TInEvent>> {
        match self.tasks.entry(id.clone()) {
            Entry::Occupied(inner) => Some(Task { inner }),
            Entry::Vacant(_) => None,
        }
    }

    /// Returns a list of all the active tasks.
    #[inline]
    pub fn tasks<'a>(&'a self) -> impl Iterator<Item = TaskId> + 'a {
        self.tasks.keys().cloned()
    }

    /// Provides an API similar to `Stream`, except that it cannot error.
    pub fn poll(&mut self) -> Async<HandledNodesEvent<TOutEvent, THandler>> {
        for to_spawn in self.to_spawn.drain() {
            tokio_executor::spawn(to_spawn);
        }
        println!("[HandledNodesTasks, poll] events_rx");
        loop {
            println!("[HandledNodesTasks, poll] events_rx –– top of the loop");
            match self.events_rx.poll() {
                Ok(Async::Ready(Some((message, task_id)))) => {
                    println!("[HandledNodesTasks, poll] events_rx – Async::Ready(Some(…)) – message, task_id={:?}", task_id);
                    // If the task id is no longer in `self.tasks`, that means that the user called
                    // `close()` on this task earlier. Therefore no new event should be generated
                    // for this task.
                    if !self.tasks.contains_key(&task_id) {
                        println!("[HandledNodesTasks, poll] events_rx – Async::Ready(Some()) – message for unknown task, task_id={:?} – NO SUCH TASK", task_id);
                        continue;
                    };

                    match message {
                        InToExtMessage::NodeEvent(event) => {
                            println!("[HandledNodesTasks, poll] events_rx – Async::Ready(Some(NodeEvent(event))) – task_id={:?}", task_id);
                            break Async::Ready(HandledNodesEvent::NodeEvent {
                                id: task_id,
                                event,
                            });
                        },
                        InToExtMessage::NodeReached(peer_id) => {
                            println!("[HandledNodesTasks, poll] events_rx – Async::Ready(Some(NodeReached(peer_id))) – task_id={:?}", task_id);
                            break Async::Ready(HandledNodesEvent::NodeReached {
                                id: task_id,
                                peer_id,
                            });
                        },
                        InToExtMessage::TaskClosed(result, handler) => {
                            println!("[HandledNodesTasks, poll] events_rx – Async::Ready(Some(TaskClosed(result))) – task_id={:?}", task_id);
                            let _ = self.tasks.remove(&task_id);
                            break Async::Ready(HandledNodesEvent::TaskClosed {
                                id: task_id, result, handler
                            });
                        },
                    }
                }
                Ok(Async::NotReady) => {
                    println!("[HandledNodesTasks, poll] events_rx – Async::NotReady");
                    break Async::NotReady;
                }
                Ok(Async::Ready(None)) => {
                    unreachable!("The sender is in self as well, therefore the receiver never \
                                  closes.")
                },
                Err(()) => unreachable!("An unbounded receiver never errors"),
            }
        }
    }
}

/// Access to a task in the collection.
pub struct Task<'a, TInEvent: 'a> {
    inner: OccupiedEntry<'a, TaskId, mpsc::UnboundedSender<TInEvent>>,
}

impl<'a, TInEvent> Task<'a, TInEvent> {
    /// Sends an event to the given node.
    // TODO: report back on delivery
    #[inline]
    pub fn send_event(&mut self, event: TInEvent) {
        // It is possible that the sender is closed if the background task has already finished
        // but the local state hasn't been updated yet because we haven't been polled in the
        // meanwhile.
        let _ = self.inner.get_mut().unbounded_send(event);
    }

    /// Returns the task id.
    #[inline]
    pub fn id(&self) -> TaskId {
        *self.inner.key()
    }

    /// Closes the task.
    ///
    /// No further event will be generated for this task.
    pub fn close(self) {
        self.inner.remove();
    }
}

impl<'a, TInEvent> fmt::Debug for Task<'a, TInEvent> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_tuple("Task")
            .field(&self.id())
            .finish()
    }
}

impl<TInEvent, TOutEvent, THandler> Stream for HandledNodesTasks<TInEvent, TOutEvent, THandler> {
    type Item = HandledNodesEvent<TOutEvent, THandler>;
    type Error = Void; // TODO: use ! once stable

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        println!("[HandledNodesTasks as Stream] poll");
        Ok(self.poll().map(Option::Some))
    }
}

/// Message to transmit from a task to the public API.
#[derive(Debug)]
enum InToExtMessage<TOutEvent, THandler> {
    /// A connection to a node has succeeded.
    NodeReached(PeerId),
    /// The task closed.
    TaskClosed(Result<(), IoError>, Option<THandler>),
    /// An event from the node.
    NodeEvent(TOutEvent),
}

/// Implementation of `Future` that handles a single node, and all the communications between
/// the various components of the `HandledNodesTasks`.
struct NodeTask<TFut, TMuxer, THandler, TInEvent, TOutEvent>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Sender to transmit events to the outside.
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TOutEvent, THandler>, TaskId)>,
    /// Receiving end for events sent from the main `HandledNodesTasks`.
    in_events_rx: stream::Fuse<mpsc::UnboundedReceiver<TInEvent>>,
    /// Inner state of the `NodeTask`.
    inner: NodeTaskInner<TFut, TMuxer, THandler, TInEvent>,
    /// Identifier of the attempt.
    id: TaskId,
}

enum NodeTaskInner<TFut, TMuxer, THandler, TInEvent>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Future to resolve to connect to the node.
    Future {
        /// The future that will attempt to reach the node.
        future: TFut,
        /// The handler that will be used to build the `HandledNode`.
        handler: THandler,
        /// While we are dialing the future, we need to buffer the events received on
        /// `in_events_rx` so that they get delivered once dialing succeeds. We can't simply leave
        /// events in `in_events_rx` because we have to detect if it gets closed.
        events_buffer: Vec<TInEvent>,
    },

    /// Fully functional node.
    Node(HandledNode<TMuxer, THandler>),

    /// A panic happened while polling.
    Poisoned,
}

impl<TFut, TMuxer, THandler, TInEvent, TOutEvent> Future for
    NodeTask<TFut, TMuxer, THandler, TInEvent, TOutEvent>
where
    TMuxer: StreamMuxer,
    TFut: Future<Item = (PeerId, TMuxer), Error = IoError>,
    THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent>,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        println!("[NodeTask, poll] START");
        loop {
            match mem::replace(&mut self.inner, NodeTaskInner::Poisoned) {
                // First possibility: we are still trying to reach a node.
                NodeTaskInner::Future { mut future, handler, mut events_buffer } => {
                    println!("[NodeTask, poll]  NodeTaskInner::Future; still connecting; polling in_events_rx to buffer up events");
                    // If self.in_events_rx is closed, we stop the task.
                    loop {
                        match self.in_events_rx.poll() {
                            Ok(Async::Ready(None)) => {
                                println!("[NodeTask, poll]    NodeTaskInner::Future; in_events_rx: Async::Ready(None) – stream is 'finished'; shouldn't be polled again");
                                return Ok(Async::Ready(()))
                            },
                            Ok(Async::Ready(Some(event))) => {
                                println!("[NodeTask, poll]    NodeTaskInner::Future; in_events_rx: Async::Ready(Some) – buffering up an event");
                                events_buffer.push(event)
                            },
                            Ok(Async::NotReady) => {
                                println!("[NodeTask, poll]    NodeTaskInner::Future; in_events_rx: Async::NotReady – break, no more events to read off the channel");
                                break
                            },
                            Err(_) => unreachable!("An UnboundedReceiver never errors"),
                        }
                    }
                    println!("[NodeTask, poll]  NodeTaskInner::Future; polling future");
                    // Check whether dialing succeeded.
                    match future.poll() {
                        Ok(Async::Ready((peer_id, muxer))) => {
                            println!("[NodeTask, poll]      NodeTaskInner::Future: AsyncReady(Some) – setting up HandledNode and sending NodeReached");
                            let event = InToExtMessage::NodeReached(peer_id);
                            let mut node = HandledNode::new(muxer, handler);
                            for event in events_buffer {
                                println!("[NodeTask, poll]      NodeTaskInner::Future: AsyncReady(Some) – injecting event");
                                node.inject_event(event);
                            }
                            if let Err(e) = self.events_tx.unbounded_send((event, self.id)) {
                                println!("[NodeTask, poll]          NodeTaskInner::Future: AsyncReady(Some) – Error sending NodeReached={:?}", e);
                                node.shutdown();
                            }
                            self.inner = NodeTaskInner::Node(node);
                        }
                        Ok(Async::NotReady) => {
                            println!("[NodeTask, poll]      NodeTaskInner::Future: Async::NotReady – creating a new NodeTaskInner and putting it back in the NodeTask");
                            self.inner = NodeTaskInner::Future { future, handler, events_buffer };
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            println!("[NodeTask, poll]      NodeTaskInner::Future: Err");
                            // End the task
                            let event = InToExtMessage::TaskClosed(Err(err), Some(handler));
                            let _ = self.events_tx.unbounded_send((event, self.id));
                            return Ok(Async::Ready(()));
                        }
                    }
                },

                // Second possibility: we have a node.
                NodeTaskInner::Node(mut node) => {
                    println!("[NodeTask, poll]  NodeTaskInner::Node; we have a HandledNode");
                    // Start by handling commands received from the outside of the task.
                    if !self.in_events_rx.is_done() {
                        println!("[NodeTask, poll]      NodeTaskInner::Node; incoming events chan is open");
                        loop {
                            match self.in_events_rx.poll() {
                                Ok(Async::NotReady) => {
                                    println!("[NodeTask, poll]          NodeTaskInner::Node; in_events_rx Async::NotReady – done reading incoming events");
                                    break
                                },
                                Ok(Async::Ready(Some(event))) => {
                                    println!("[NodeTask, poll]          NodeTaskInner::Node; in_events_rx Async::Ready(Some) – there was an event sent from the outside, injecting");
                                    node.inject_event(event);
                                },
                                Ok(Async::Ready(None)) => {
                                    println!("[NodeTask, poll]          NodeTaskInner::Node; in_events_rx Async::Ready(None) – calling shutdown()");
                                    // Node closed by the external API ; start shutdown process.
                                    node.shutdown();
                                    break;
                                }
                                Err(()) => unreachable!("An unbounded receiver never errors"),
                            }
                        }
                    } else {
                        println!("[NodeTask, poll]      NodeTaskInner::Node; incoming events chan is CLOSED");
                    }

                    // Process the node.
                    loop {
                        println!("[NodeTask, poll]  NodeTaskInner::Node; polling node, top of loop");
                        match node.poll() {
                            // REVIEW: I don't think this can happen, see comment in handled_node.rs
                            Ok(Async::NotReady) => {
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polled node; Async::NotReady – putting back the NodeTaskInner::Node and returning Async::NotReady");
                                self.inner = NodeTaskInner::Node(node);
                                return Ok(Async::NotReady);
                            },
                            Ok(Async::Ready(Some(event))) => {
                                // The only possible event here is a NodeHandlerEvent::Custom(event)
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polled handled node; Async::Ready(Some) –– sending a NodeEvent on events_tx and keep looping<––––");
                                let event = InToExtMessage::NodeEvent(event);
                                if let Err(e) = self.events_tx.unbounded_send((event, self.id)) {
                                    println!("[NodeTask, poll]          NodeTaskInner::Node; polled node; Async::Ready(Some); error sending on events_tx channel={:?}. Shutting down the node.", e);
                                    node.shutdown();
                                }
                            }
                            Ok(Async::Ready(None)) => {
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polled node; Async::Ready(None) – sending TaskClosed and returning Async::Ready(())");
                                let event = InToExtMessage::TaskClosed(Ok(()), None);
                                let _ = self.events_tx.unbounded_send((event, self.id));
                                return Ok(Async::Ready(())); // End the task.
                            }
                            Err(err) => {
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polled node; Err");
                                let event = InToExtMessage::TaskClosed(Err(err), None);
                                let _ = self.events_tx.unbounded_send((event, self.id));
                                return Ok(Async::Ready(())); // End the task.
                            }
                        }
                    }
                },

                // This happens if a previous poll has ended unexpectedly. The API of futures
                // guarantees that we shouldn't be polled again.
                NodeTaskInner::Poisoned => panic!("the node task panicked or errored earlier")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    mod task {
        use super::super::*;
        use std::collections::{HashMap, hash_map::Entry};

        #[derive(Debug)]
        enum InEvent{Banana}

        #[test]
        fn send_event() {
            let mut dict = HashMap::new();
            let (tx, mut rx) = mpsc::unbounded::<InEvent>();
            let id = TaskId(123);
            dict.insert(id, tx);
            let mut task = match dict.entry(id) {
                Entry::Occupied(inner) => Task{inner},
                _ => unreachable!()
            };

            task.send_event(InEvent::Banana);
            assert_matches!(rx.poll(), Ok(Async::Ready(Some(InEvent::Banana))));
        }

        #[test]
        fn id() {
            let mut dict = HashMap::new();
            let (tx, _) = mpsc::unbounded::<InEvent>();
            let id = TaskId(123);
            dict.insert(id, tx);
            let task = match dict.entry(id) {
                Entry::Occupied(inner) => Task{inner},
                _ => unreachable!()
            };

            assert_eq!(id, task.id())
        }

        #[test]
        #[ignore]
        fn close() {
            let mut dict = HashMap::new();
            let (tx, _) = mpsc::unbounded::<InEvent>();
            let id = TaskId(123);
            dict.insert(id, tx);
            let task = match dict.entry(id) {
                Entry::Occupied(inner) => Task{inner},
                _ => unreachable!()
            };

            task.close();
            // REVIEW: this doesn't work because the value is moved. I'd argue
            // this doesn't need to be tested as it's enforced at compile time.
            // assert!(!dict.contains_key(&id));
        }
    }

    mod node_task {
        use super::super::*;
        use futures::future::{self, FutureResult};
        use futures::sync::mpsc::{UnboundedReceiver, UnboundedSender};
        use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
        use tests::dummy_handler::{Handler, InEvent, OutEvent};
        use rand::random;
        use {PeerId, PublicKey};
        use std::io;
        use tokio::runtime::current_thread::Runtime;

        type TestNodeTask = NodeTask<
            FutureResult<(PeerId, DummyMuxer), IoError>,
            DummyMuxer,
            Handler,
            InEvent,
            OutEvent,
        >;

        struct TestBuilder {
           task_id: TaskId,
           inner_fut: Option<FutureResult<(PeerId, DummyMuxer), IoError>>,
        }
        impl TestBuilder {
            fn new() -> Self {
                TestBuilder {
                    task_id: TaskId(123),
                    inner_fut: {
                        let peer_id = PublicKey::Rsa((0 .. 2048).map(|_| -> u8 { random() }).collect()).into_peer_id();
                        Some(future::ok((peer_id, DummyMuxer::new())))
                    },
                }
            }

            fn with_inner_fut(&mut self, fut: FutureResult<(PeerId, DummyMuxer), IoError>) -> &mut Self{
                self.inner_fut = Some(fut);
                self
            }

            fn with_task_id(&mut self, id: usize) -> &mut Self {
                self.task_id = TaskId(id);
                self
            }

            fn node_task(&mut self) -> (
                TestNodeTask,
                UnboundedSender<InEvent>,
                UnboundedReceiver<(InToExtMessage<OutEvent, Handler>, TaskId)>,
            ) {
                let (events_from_node_task_tx, events_from_node_task_rx) = mpsc::unbounded::<(InToExtMessage<OutEvent, Handler>, TaskId)>();
                let (events_to_node_task_tx, events_to_node_task_rx) = mpsc::unbounded::<InEvent>();
                let inner = NodeTaskInner::Future {
                        future: self.inner_fut.take().unwrap(),
                        handler: Handler::default(),
                        events_buffer: Vec::new(),
                    };
                let node_task = NodeTask {
                    inner: inner,
                    events_tx: events_from_node_task_tx.clone(), // events TO the outside
                    in_events_rx: events_to_node_task_rx.fuse(), // events FROM the outside
                    id: self.task_id,
                };
                (node_task, events_to_node_task_tx, events_from_node_task_rx)
            }
        }

        #[test]
        fn task_emits_event_when_things_happen_in_the_node() {
            let (node_task, tx, mut rx) = TestBuilder::new()
                .with_task_id(890)
                .node_task();

            tx.unbounded_send(InEvent::Custom("beef")).expect("send to NodeTask should work");

            let mut rt = Runtime::new().unwrap();
            rt.spawn(node_task);
            let events = rt.block_on(rx.by_ref().take(2).collect()).expect("reading on rx should work");

            assert_matches!(events[0], (InToExtMessage::NodeReached(_), TaskId(890)));
            assert_matches!(events[1], (InToExtMessage::NodeEvent(ref outevent), TaskId(890)) => {
                assert_matches!(outevent, OutEvent::Custom(beef) => {
                    assert_eq!(beef, &"beef");
                })
            });
        }

        #[test]
        fn task_exits_when_node_errors() {
            let mut rt = Runtime::new().unwrap();
            let (node_task, _tx, rx) = TestBuilder::new()
                .with_inner_fut(future::err(io::Error::new(io::ErrorKind::Other, "nah")))
                .with_task_id(345)
                .node_task();

            rt.spawn(node_task);
            let events = rt.block_on(rx.collect()).expect("rx failed");
            println!("[test] poll result events={:?}", events);
            assert!(events.len() == 1);
            assert_matches!(events[0], (InToExtMessage::TaskClosed{..}, TaskId(345)));
        }

        #[test]
        fn task_exits_when_node_is_done() {
            let mut rt = Runtime::new().unwrap();
            let fut = {
                let peer_id = PublicKey::Rsa((0 .. 2048).map(|_| -> u8 { random() }).collect()).into_peer_id();
                let mut muxer = DummyMuxer::new();
                muxer.set_inbound_connection_state(DummyConnectionState::Closed);
                muxer.set_outbound_connection_state(DummyConnectionState::Closed);
                future::ok((peer_id, muxer))
            };
            let (node_task, tx, rx) = TestBuilder::new()
                .with_inner_fut(fut)
                .with_task_id(345)
                .node_task();

            // Even though we set up the muxer outbound state to be `Closed` we
            // still need to open a substream or the outbound state will never
            // be checked.
            // We do not have a HandledNode yet, so we can't simply call
            // `open_substream`. Instead we send a message to the NodeTask,
            // which will be buffered until the inner future resolves, then
            // it'll call `inject_event` on the handler. In the test Handler,
            // inject_event will set the next state so that it yields an
            // OutboundSubstreamRequest.
            // Back up in the HandledNode, at the next iteration we'll
            // open_substream() and iterate again. This time, node.poll() will
            // poll the muxer inbound (closed) and also outbound (because now
            // there's an entry in the outbound_streams) which will be Closed
            // (because we set up the muxer state so) and thus yield
            // Async::Ready(None) which in turn makes the NodeStream yield an
            // Async::Ready(OutboundClosed) to the HandledNode.
            // Now we're at the point where poll_inbound, poll_outbound and
            // address are all skipped and there is nothing left to do: we yield
            // Async::Ready(None) from the NodeStream. In the HandledNode,
            // Async::Ready(None) triggers a shutdown of the Handler so that it
            // also yields Async::Ready(None). Finally, the NodeTask gets a
            // Async::Ready(None) and sends a TaskClosed and returns
            // Async::Ready(()). Qed.

            let create_outbound_substream_event = InEvent::Substream(Some(135));
            tx.unbounded_send(create_outbound_substream_event).expect("send msg works");
            rt.spawn(node_task);
            let events = rt.block_on(rx.collect()).expect("rx failed");

            assert_eq!(events.len(), 2);
            assert_matches!(events[0].0, InToExtMessage::NodeReached(PeerId{..}));
            assert_matches!(events[1].0, InToExtMessage::TaskClosed(Ok(()), _));
        }
    }
    mod handled_node_tasks {
        use super::super::*;
        use futures::future::{self};
        use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
        use tests::dummy_handler::{Handler, HandlerState, InEvent, OutEvent};
        use rand::random;
        use PublicKey;
        use tokio::runtime::Builder;
        use tokio::runtime::current_thread::Runtime;
        use nodes::handled_node::NodeHandlerEvent;

        type TestHandledNodesTasks = HandledNodesTasks<InEvent, OutEvent, Handler>;
        struct TestBuilder {
            muxer: DummyMuxer,
            handler: Handler,
            task_count: usize,
        }
        impl TestBuilder {
            fn new() -> Self {
                TestBuilder {
                    muxer: DummyMuxer::new(),
                    handler: Handler::default(),
                    task_count: 0,
                }
            }

            fn with_tasks(&mut self, amt: usize) -> &mut Self {
                self.task_count = amt;
                self
            }
            fn with_muxer_inbound_state(&mut self, state: DummyConnectionState) -> &mut Self {
                self.muxer.set_inbound_connection_state(state);
                self
            }
            fn with_muxer_outbound_state(&mut self, state: DummyConnectionState) -> &mut Self {
                self.muxer.set_outbound_connection_state(state);
                self
            }
            fn with_handler_state(&mut self, state: HandlerState) -> &mut Self {
                self.handler.state = Some(state);
                self
            }
            fn with_handler_states(&mut self, states: Vec<HandlerState>) -> &mut Self {
                self.handler.next_states = states;
                self
            }
            fn handled_nodes_tasks(&mut self) -> (TestHandledNodesTasks, Vec<TaskId>) {
                let mut handled_nodes = HandledNodesTasks::new();
                let peer_id = PublicKey::Rsa((0 .. 2048).map(|_| -> u8 { random() }).collect()).into_peer_id();
                let mut task_ids = Vec::new();
                for _i in 0..self.task_count {
                    let fut = future::ok((peer_id.clone(), self.muxer.clone()));
                    task_ids.push(
                        handled_nodes.add_reach_attempt(fut, self.handler.clone())
                    );
                }
                (handled_nodes, task_ids)
            }
        }
        #[test]
        fn query_for_tasks() {
            let (mut handled_nodes, task_ids) = TestBuilder::new()
                .with_tasks(3)
                .handled_nodes_tasks();

            assert_eq!(task_ids.len(), 3);
            assert_eq!(handled_nodes.task(TaskId(2)).unwrap().id(), task_ids[2]);
            assert!(handled_nodes.task(TaskId(545534)).is_none());
        }
        #[test]
        fn iterate_over_all_tasks() {
            let (handled_nodes, task_ids) = TestBuilder::new()
                .with_tasks(3)
                .handled_nodes_tasks();

            let mut tasks: Vec<TaskId> = handled_nodes.tasks().collect();
            assert!(tasks.len() == 3);
            tasks.sort_by_key(|t| t.0 );
            assert_eq!(tasks, task_ids);
        }

        #[test]
        fn add_reach_attempt_prepares_a_new_task() {
            let mut handled_nodes = HandledNodesTasks::new();
            assert_eq!(handled_nodes.tasks().count(), 0);
            assert_eq!(handled_nodes.to_spawn.len(), 0);

            handled_nodes.add_reach_attempt( future::empty(), Handler::default() );

            assert_eq!(handled_nodes.tasks().count(), 1);
            assert_eq!(handled_nodes.to_spawn.len(), 1);
        }

        #[test]
        fn running_handled_tasks_reaches_the_nodes() {
            let (mut handled_nodes_tasks, _) = TestBuilder::new()
                .with_tasks(5)
                .with_muxer_inbound_state(DummyConnectionState::Closed)
                .with_muxer_outbound_state(DummyConnectionState::Closed)
                .with_handler_state(HandlerState::Err) // stop the loop
                .handled_nodes_tasks();

            let mut rt = Runtime::new().unwrap();
            let mut events: (Option<HandledNodesEvent<_,_>>, TestHandledNodesTasks);
            // we're running on a single thread so events are sequential: first
            // we get a NodeReached, then a TaskClosed
            for i in 0..5 {
                events = rt.block_on(handled_nodes_tasks.into_future()).unwrap();
                assert_matches!(events, (Some(HandledNodesEvent::NodeReached{..}), ref hnt) => {
                    assert_matches!(hnt, HandledNodesTasks{..});
                    assert_eq!(hnt.tasks().count(), 5 - i);
                });
                handled_nodes_tasks = events.1;
                events = rt.block_on(handled_nodes_tasks.into_future()).unwrap();
                assert_matches!(events, (Some(HandledNodesEvent::TaskClosed{..}), _));
                handled_nodes_tasks = events.1;
            }
        }

        #[test]
        fn events_in_tasks_are_emitted() {
            // States are pop()'d so they are set in reverse order by the Handler
            let handler_states = vec![
                HandlerState::Err,
                HandlerState::Ready(Some(NodeHandlerEvent::Custom(OutEvent::Custom("from handler2") ))),
                HandlerState::Ready(Some(NodeHandlerEvent::Custom(OutEvent::Custom("from handler") ))),
            ];

            let (mut handled_nodes_tasks, _) = TestBuilder::new()
                .with_tasks(1)
                .with_muxer_inbound_state(DummyConnectionState::Pending)
                .with_muxer_outbound_state(DummyConnectionState::Opened)
                .with_handler_states(handler_states)
                .handled_nodes_tasks();

            let tx = {
                let mut task0 = handled_nodes_tasks.task(TaskId(0)).unwrap();
                let tx = task0.inner.get_mut();
                tx.clone()
            };

            let mut rt = Builder::new().core_threads(4).build().unwrap();
            let mut events = rt.block_on(handled_nodes_tasks.into_future()).unwrap();
            assert_matches!(events.0.unwrap(), HandledNodesEvent::NodeReached{..});

            tx.unbounded_send(InEvent::NextState).expect("send works");
            events = rt.block_on(events.1.into_future()).unwrap();
            assert_matches!(events.0.unwrap(), HandledNodesEvent::NodeEvent{id: _, event} => {
                assert_matches!(event, OutEvent::Custom("from handler"));
            });

            tx.unbounded_send(InEvent::NextState).expect("send works");
            events = rt.block_on(events.1.into_future()).unwrap();
            assert_matches!(events.0.unwrap(), HandledNodesEvent::NodeEvent{id: _, event} => {
                assert_matches!(event, OutEvent::Custom("from handler2"));
            });

            tx.unbounded_send(InEvent::NextState).expect("send works");
            events = rt.block_on(events.1.into_future()).unwrap();
            assert_matches!(events.0.unwrap(), HandledNodesEvent::TaskClosed{id: _, result, handler: _} => {
                assert_matches!(result, Err(_));
            });
        }
    }
}
