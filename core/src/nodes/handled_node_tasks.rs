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
use {Multiaddr, PeerId};

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
pub struct HandledNodesTasks<TInEvent, TOutEvent> {
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
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TOutEvent>, TaskId)>,

    /// Receiver side for the events.
    events_rx: mpsc::UnboundedReceiver<(InToExtMessage<TOutEvent>, TaskId)>,
}

impl<TInEvent, TOutEvent> fmt::Debug for HandledNodesTasks<TInEvent, TOutEvent> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_list()
            .entries(self.tasks.keys().cloned())
            .finish()
    }
}

/// Event that can happen on the `HandledNodesTasks`.
#[derive(Debug)]
pub enum HandledNodesEvent<TOutEvent> {
    /// A task has been closed.
    ///
    /// This happens once the node handler closes or an error happens.
    TaskClosed {
        /// Identifier of the task that closed.
        id: TaskId,
        /// What happened.
        result: Result<(), IoError>,
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

impl<TInEvent, TOutEvent> HandledNodesTasks<TInEvent, TOutEvent> {
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
    pub fn add_reach_attempt<TFut, TMuxer, TAddrFut, THandler>(&mut self, future: TFut, handler: THandler)
        -> TaskId
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
        task_id
    }

    /// Sends an event to all the tasks, including the pending ones.
    pub fn broadcast_event(&mut self, event: &TInEvent)
    where TInEvent: Clone,
    {
        for sender in self.tasks.values() {
            // Note: it is possible that sending an event fails if the background task has already
            // finished, but the local state hasn't reflected that yet becaues it hasn't been
            // polled. This is not an error situation.
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
    pub fn poll(&mut self) -> Async<Option<HandledNodesEvent<TOutEvent>>> {
        for to_spawn in self.to_spawn.drain() {
            tokio_executor::spawn(to_spawn);
        }

        loop {
            match self.events_rx.poll() {
                Ok(Async::Ready(Some((message, task_id)))) => {
                    // If the task id is no longer in `self.tasks`, that means that the user called
                    // `close()` on this task earlier. Therefore no new event should be generated
                    // for this task.
                    if !self.tasks.contains_key(&task_id) {
                        continue;
                    };

                    match message {
                        InToExtMessage::NodeEvent(event) => {
                            break Async::Ready(Some(HandledNodesEvent::NodeEvent {
                                id: task_id,
                                event,
                            }));
                        },
                        InToExtMessage::NodeReached(peer_id) => {
                            break Async::Ready(Some(HandledNodesEvent::NodeReached {
                                id: task_id,
                                peer_id,
                            }));
                        },
                        InToExtMessage::TaskClosed(result) => {
                            let _ = self.tasks.remove(&task_id);
                            break Async::Ready(Some(HandledNodesEvent::TaskClosed {
                                id: task_id, result
                            }));
                        },
                    }
                }
                Ok(Async::NotReady) => {
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

impl<TInEvent, TOutEvent> Stream for HandledNodesTasks<TInEvent, TOutEvent> {
    type Item = HandledNodesEvent<TOutEvent>;
    type Error = Void; // TODO: use ! once stable

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        Ok(self.poll())
    }
}

/// Message to transmit from a task to the public API.
#[derive(Debug)]
enum InToExtMessage<TOutEvent> {
    /// A connection to a node has succeeded.
    NodeReached(PeerId),
    /// The task closed.
    TaskClosed(Result<(), IoError>),
    /// An event from the node.
    NodeEvent(TOutEvent),
}

/// Implementation of `Future` that handles a single node, and all the communications between
/// the various components of the `HandledNodesTasks`.
struct NodeTask<TFut, TMuxer, TAddrFut, THandler, TInEvent, TOutEvent>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
{
    /// Sender to transmit events to the outside.
    events_tx: mpsc::UnboundedSender<(InToExtMessage<TOutEvent>, TaskId)>,
    /// Receiving end for events sent from the main `HandledNodesTasks`.
    in_events_rx: stream::Fuse<mpsc::UnboundedReceiver<TInEvent>>,
    /// Inner state of the `NodeTask`.
    inner: NodeTaskInner<TFut, TMuxer, TAddrFut, THandler, TInEvent>,
    /// Identifier of the attempt.
    id: TaskId,
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
        /// The handler that will be used to build the `HandledNode`.
        handler: THandler,
        /// While we are dialing the future, we need to buffer the events received on
        /// `in_events_rx` so that they get delivered once dialing succeeds. We can't simply leave
        /// events in `in_events_rx` because we have to detect if it gets closed.
        events_buffer: Vec<TInEvent>,
    },

    /// Fully functional node.
    Node(HandledNode<TMuxer, TAddrFut, THandler>),

    /// A panic happened while polling.
    Poisoned,
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
                                println!("[NodeTask, poll]    NodeTaskInner::Future; in_events_rx: Async::Ready(None)");
                                return Ok(Async::Ready(()))
                            },
                            Ok(Async::Ready(Some(event))) => {
                                println!("[NodeTask, poll]    NodeTaskInner::Future; in_events_rx: Async::Ready(Some)");
                                events_buffer.push(event)
                            },
                            Ok(Async::NotReady) => {
                                println!("[NodeTask, poll]    NodeTaskInner::Future; in_events_rx: Async::NotReady – break");
                                break
                            },
                            Err(_) => unreachable!("An UnboundedReceiver never errors"),
                        }
                    }
                    println!("[NodeTask, poll]  NodeTaskInner::Future; polling future");
                    // Check whether dialing succeeded.
                    match future.poll() {
                        Ok(Async::Ready(((peer_id, muxer), addr_fut))) => {
                            println!("[NodeTask, poll]      NodeTaskInner::Future: AsyncReady(Some)");
                            let event = InToExtMessage::NodeReached(peer_id);
                            let mut node = HandledNode::new(muxer, addr_fut, handler);
                            for event in events_buffer {
                                node.inject_event(event);
                            }
                            if let Err(_) = self.events_tx.unbounded_send((event, self.id)) {
                                node.shutdown();
                            }
                            self.inner = NodeTaskInner::Node(node);
                        }
                        Ok(Async::NotReady) => {
                            println!("[NodeTask, poll]      NodeTaskInner::Future: Async::NotReady");
                            self.inner = NodeTaskInner::Future { future, handler, events_buffer };
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            println!("[NodeTask, poll]      NodeTaskInner::Future: Err");
                            // End the task
                            let event = InToExtMessage::TaskClosed(Err(err));
                            let _ = self.events_tx.unbounded_send((event, self.id));
                            return Ok(Async::Ready(()));
                        }
                    }
                },

                // Second possibility: we have a node.
                NodeTaskInner::Node(mut node) => {
                    println!("[NodeTask, poll]  NodeTaskInner::Node; connected");
                    // Start by handling commands received from the outside of the task.
                    if !self.in_events_rx.is_done() {
                        println!("[NodeTask, poll]  NodeTaskInner::Node; incoming events chan is open");
                        loop {
                            match self.in_events_rx.poll() {
                                Ok(Async::NotReady) => {
                                    println!("[NodeTask, poll]      NodeTaskInner::Node; in_events_rx Async::NotReady");
                                    break
                                },
                                Ok(Async::Ready(Some(event))) => {
                                    println!("[NodeTask, poll]      NodeTaskInner::Node; in_events_rx Async::Ready(Some)");
                                    node.inject_event(event);
                                },
                                Ok(Async::Ready(None)) => {
                                    println!("[NodeTask, poll]      NodeTaskInner::Node; in_events_rx Async::Ready(None), calling shutdown()");
                                    // Node closed by the external API ; start shutdown process.
                                    node.shutdown();
                                    break;
                                }
                                Err(()) => unreachable!("An unbounded receiver never errors"),
                            }
                        }
                    }

                    // Process the node.
                    loop {
                        println!("[NodeTask, poll]      NodeTaskInner::Node; polling node, top of loop");
                        match node.poll() {
                            Ok(Async::NotReady) => {
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polling node; Async::NotReady");
                                self.inner = NodeTaskInner::Node(node);
                                return Ok(Async::NotReady);
                            },
                            Ok(Async::Ready(Some(event))) => {
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polling node; Async::Ready(Some) xxx");
                                let event = InToExtMessage::NodeEvent(event);
                                if let Err(_) = self.events_tx.unbounded_send((event, self.id)) {
                                    println!("[NodeTask, poll]      NodeTaskInner::Node; polling node; Async::Ready(Some); error sending on events_tx channel. Shutting down the node.");
                                    node.shutdown();
                                }
                                println!("[NodeTask, poll]      NodeTaskInner::Node; sent a message back");
                            }
                            Ok(Async::Ready(None)) => {
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polling node; Async::Ready(None)");
                                let event = InToExtMessage::TaskClosed(Ok(()));
                                let _ = self.events_tx.unbounded_send((event, self.id));
                                return Ok(Async::Ready(())); // End the task.
                            }
                            Err(err) => {
                                println!("[NodeTask, poll]      NodeTaskInner::Node; polling node; Err");
                                let event = InToExtMessage::TaskClosed(Err(err));
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
        use tests::dummy_muxer::DummyMuxer;
        use tests::dummy_handler::{Handler, InEvent, OutEvent};
        use rand::random;
        use {PeerId, PublicKey};
        use std::io;

        type TestNodeTask = NodeTask<
            FutureResult<((PeerId, DummyMuxer), FutureResult<Multiaddr, IoError>), IoError>,
            DummyMuxer,
            FutureResult<Multiaddr, IoError>,
            Handler,
            InEvent,
            OutEvent,
        >;
        fn build_node_task() -> (
            TestNodeTask,
            UnboundedSender<InEvent>,
            UnboundedReceiver<(InToExtMessage<OutEvent>, TaskId)>,
        )
        {
            let id = TaskId(123);
            let (events_from_node_task_tx, events_from_node_task_rx) = mpsc::unbounded::<(InToExtMessage<OutEvent>, TaskId)>();
            let (events_to_node_task_tx, events_to_node_task_rx) = mpsc::unbounded::<InEvent>();
            let handler = Handler::default();
            let peer_id = PublicKey::Rsa((0 .. 2048).map(|_| -> u8 { random() }).collect()).into_peer_id();
            let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
            let addr_fut = future::ok(addr);
            let fut = future::ok(((peer_id, DummyMuxer::new()), addr_fut));
            let node_task = NodeTask {
                inner: NodeTaskInner::Future {
                    future: fut,
                    handler: handler,
                    events_buffer: Vec::new(),
                },
                events_tx: events_from_node_task_tx.clone(), // events TO the outside
                in_events_rx: events_to_node_task_rx.fuse(), // events FROM the outside
                id
            };
            (node_task, events_to_node_task_tx, events_from_node_task_rx)
        }

        // poll first checks if we're ready to do work
        // if we're still connecting check the incoming events and queue them up for later execution
        // then check the connection: still connecting? yes => return; no => send NodeReached event and keep working
        // If we have reached the node, take care of incoming events we buffered up while waiting
        // When the node is connected, take care of incoming events

        // #[test]
        // fn poll() {
        //     use tokio::runtime::current_thread::Runtime;
        //     let (mut node_task, to_node_tx, mut from_node_rx) = build_node_task();
        //     let mut rt = Runtime::new().unwrap();
        //     // rt.spawn(node_task);
        //     to_node_tx.unbounded_send(InEvent::Custom("beef")).unwrap();
        //     let mut events_seen = Vec::new();
        //     {
        //         let from_node_rx_fut = from_node_rx.take(2).for_each(|event| {
        //             println!("––––> THE NODE SENT {:?}", event);
        //             events_seen.push(event);
        //             Ok(())
        //         });
        //         // rt.block_on(from_node_rx_fut);
        //         let common_fut = from_node_rx_fut.select2(node_task);
        //         rt.block_on(common_fut);
        //     }

        //     assert_matches!(events_seen[0], (InToExtMessage::NodeReached(_), TaskId(123)));
        //     assert_matches!(events_seen[1], (InToExtMessage::NodeEvent(ref outevent), TaskId(123)) => {
        //         assert_matches!(outevent, OutEvent::Custom(beef) => {
        //             assert_eq!(beef, &"beef");
        //         })
        //     });
        // }

        #[test]
        fn poll() {
            let (mut node_task, to_node_tx, mut from_node_rx) = build_node_task();

            let node_thread = std::thread::spawn(move || {
                to_node_tx.unbounded_send(InEvent::Custom("beef")).unwrap();
                tokio::runtime::current_thread::block_on_all(node_task)
            });

            // Want to assert on the messages sent on `from_node_rx`
            let mut events_seen = Vec::new();
            {
                let from_node_rx_fut = from_node_rx.take(2).for_each(|event| {
                    println!("––––> THE NODE SENT {:?}", event);
                    events_seen.push(event);
                    Ok(())
                });
                tokio::runtime::current_thread::block_on_all(from_node_rx_fut);
            }

            assert_matches!(events_seen[0], (InToExtMessage::NodeReached(_), TaskId(123)));
            assert_matches!(events_seen[1], (InToExtMessage::NodeEvent(ref outevent), TaskId(123)) => {
                assert_matches!(outevent, OutEvent::Custom(beef) => {
                    assert_eq!(beef, &"beef");
                    // panic!("DONE. ALL GOOD");
                })
            });

            node_thread.join().unwrap().unwrap();
        }


        // #[test]
        // fn poll() {
        //     // use tokio::runtime::current_thread::Runtime;
        //     use std::thread;
        //     let (mut node_task, to_node_tx, mut from_node_rx) = build_node_task();

        //     let node_thread = thread::spawn(move || {
        //         to_node_tx.unbounded_send(InEvent::Custom("beef")).unwrap();
        //         tokio::runtime::current_thread::block_on_all(node_task)
        //     });

        //     // Want to assert on the message sent on `from_node_rx`
        //     let mut events_seen = Vec::new();
        //     {
        //         let from_node_rx_fut = from_node_rx.take(2).for_each(|event| {
        //             println!("––––> THE NODE SENT {:?}", event);
        //             events_seen.push(event);
        //             // Ok(event)
        //             Ok(())
        //             // event.into()
        //             // assert_matches!(event, (InToExtMessage::NodeReached(_), TaskId(123)));
        //             // assert_matches!(event, (InToExtMessage::NodeEvent(outevent), TaskId(123)) => {
        //             //     println!("[test] matched");
        //             //     // assert_matches(outevent, OutEvent::Custom(beef) => {
        //             //     //     assert_eq!(beef, "beef")
        //             //     // })
        //             // } );
        //             // Ok(())
        //             // Err(())
        //         });
        //         tokio::runtime::current_thread::block_on_all(from_node_rx_fut);
        //         from_node_rx.close();
        //     }
        //     println!("\nEVENTS READ FROM NODE = {:?}", &events_seen);
        //     // to_node_tx.unbounded_send(InEvent::Shutdown);
        //     node_thread.join().unwrap().unwrap();
        // }
    }

    mod handled_node_tasks {

    }
}
