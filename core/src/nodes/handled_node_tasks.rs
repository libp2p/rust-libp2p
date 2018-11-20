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

use crate::{
    PeerId,
    muxing::StreamMuxer,
    nodes::{
        handled_node::{HandledNode, NodeHandler},
        node::Substream
    }
};
use fnv::FnvHashMap;
use futures::{prelude::*, stream, sync::mpsc};
use smallvec::SmallVec;
use std::{
    collections::hash_map::{Entry, OccupiedEntry},
    fmt,
    io::{self, Error as IoError},
    mem
};
use tokio_executor;
use void::Void;

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
    pub fn add_reach_attempt<TFut, TMuxer>(&mut self, future: TFut, handler: THandler) -> TaskId
    where
        TFut: Future<Item = (PeerId, TMuxer)> + Send + 'static,
        TFut::Error: std::error::Error + Send + Sync + 'static,
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
        task_id
    }

    /// Sends an event to all the tasks, including the pending ones.
    pub fn broadcast_event(&mut self, event: &TInEvent)
    where TInEvent: Clone,
    {
        for sender in self.tasks.values() {
            // Note: it is possible that sending an event fails if the background task has already
            // finished, but the local state hasn't reflected that yet because it hasn't been
            // polled. This is not an error situation.
            let _ = sender.unbounded_send(event.clone());
        }
    }

    /// Grants access to an object that allows controlling a task of the collection.
    ///
    /// Returns `None` if the task id is invalid.
    #[inline]
    pub fn task(&mut self, id: TaskId) -> Option<Task<TInEvent>> {
        match self.tasks.entry(id) {
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
                            break Async::Ready(HandledNodesEvent::NodeEvent {
                                id: task_id,
                                event,
                            });
                        },
                        InToExtMessage::NodeReached(peer_id) => {
                            break Async::Ready(HandledNodesEvent::NodeReached {
                                id: task_id,
                                peer_id,
                            });
                        },
                        InToExtMessage::TaskClosed(result, handler) => {
                            let _ = self.tasks.remove(&task_id);
                            break Async::Ready(HandledNodesEvent::TaskClosed {
                                id: task_id, result, handler
                            });
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
    TFut: Future<Item = (PeerId, TMuxer)>,
    TFut::Error: std::error::Error + Send + Sync + 'static,
    THandler: NodeHandler<Substream = Substream<TMuxer>, InEvent = TInEvent, OutEvent = TOutEvent>,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        loop {
            match mem::replace(&mut self.inner, NodeTaskInner::Poisoned) {
                // First possibility: we are still trying to reach a node.
                NodeTaskInner::Future { mut future, handler, mut events_buffer } => {
                    // If self.in_events_rx is closed, we stop the task.
                    loop {
                        match self.in_events_rx.poll() {
                            Ok(Async::Ready(None)) => return Ok(Async::Ready(())),
                            Ok(Async::Ready(Some(event))) => events_buffer.push(event),
                            Ok(Async::NotReady) => break,
                            Err(_) => unreachable!("An UnboundedReceiver never errors"),
                        }
                    }
                    // Check whether dialing succeeded.
                    match future.poll() {
                        Ok(Async::Ready((peer_id, muxer))) => {
                            let event = InToExtMessage::NodeReached(peer_id);
                            let mut node = HandledNode::new(muxer, handler);
                            for event in events_buffer {
                                node.inject_event(event);
                            }
                            if self.events_tx.unbounded_send((event, self.id)).is_err() {
                                node.shutdown();
                            }
                            self.inner = NodeTaskInner::Node(node);
                        }
                        Ok(Async::NotReady) => {
                            self.inner = NodeTaskInner::Future { future, handler, events_buffer };
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            // End the task
                            let ioerr = IoError::new(io::ErrorKind::Other, err);
                            let event = InToExtMessage::TaskClosed(Err(ioerr), Some(handler));
                            let _ = self.events_tx.unbounded_send((event, self.id));
                            return Ok(Async::Ready(()));
                        }
                    }
                },

                // Second possibility: we have a node.
                NodeTaskInner::Node(mut node) => {
                    // Start by handling commands received from the outside of the task.
                    if !self.in_events_rx.is_done() {
                        loop {
                            match self.in_events_rx.poll() {
                                Ok(Async::NotReady) => break,
                                Ok(Async::Ready(Some(event))) => {
                                    node.inject_event(event)
                                },
                                Ok(Async::Ready(None)) => {
                                    // Node closed by the external API; start shutdown process.
                                    node.shutdown();
                                    break;
                                }
                                Err(()) => unreachable!("An unbounded receiver never errors"),
                            }
                        }
                    }

                    // Process the node.
                    loop {
                        match node.poll() {
                            Ok(Async::NotReady) => {
                                self.inner = NodeTaskInner::Node(node);
                                return Ok(Async::NotReady);
                            },
                            Ok(Async::Ready(Some(event))) => {
                                let event = InToExtMessage::NodeEvent(event);
                                if self.events_tx.unbounded_send((event, self.id)).is_err() {
                                    node.shutdown();
                                }
                            }
                            Ok(Async::Ready(None)) => {
                                let event = InToExtMessage::TaskClosed(Ok(()), None);
                                let _ = self.events_tx.unbounded_send((event, self.id));
                                return Ok(Async::Ready(())); // End the task.
                            }
                            Err(err) => {
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
    use super::*;

    use std::io;

    use futures::future::{self, FutureResult};
    use futures::sync::mpsc::{UnboundedReceiver, UnboundedSender};
    use nodes::handled_node::NodeHandlerEvent;
    use tests::dummy_handler::{Handler, HandlerState, InEvent, OutEvent, TestHandledNode};
    use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
    use tokio::runtime::Builder;
    use tokio::runtime::current_thread::Runtime;
    use void::Void;
    use PeerId;

    type TestNodeTask = NodeTask<
        FutureResult<(PeerId, DummyMuxer), IoError>,
        DummyMuxer,
        Handler,
        InEvent,
        OutEvent,
    >;

    struct NodeTaskTestBuilder {
       task_id: TaskId,
       inner_node: Option<TestHandledNode>,
       inner_fut: Option<FutureResult<(PeerId, DummyMuxer), IoError>>,
    }

    impl NodeTaskTestBuilder {
        fn new() -> Self {
            NodeTaskTestBuilder {
                task_id: TaskId(123),
                inner_node: None,
                inner_fut: {
                    let peer_id = PeerId::random();
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
            let inner = if self.inner_node.is_some() {
                NodeTaskInner::Node(self.inner_node.take().unwrap())
            } else {
                NodeTaskInner::Future {
                    future: self.inner_fut.take().unwrap(),
                    handler: Handler::default(),
                    events_buffer: Vec::new(),
                }
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

    type TestHandledNodesTasks = HandledNodesTasks<InEvent, OutEvent, Handler>;

    struct HandledNodeTaskTestBuilder {
        muxer: DummyMuxer,
        handler: Handler,
        task_count: usize,
    }

    impl HandledNodeTaskTestBuilder {
        fn new() -> Self {
            HandledNodeTaskTestBuilder {
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
            let peer_id = PeerId::random();
            let mut task_ids = Vec::new();
            for _i in 0..self.task_count {
                let fut = future::ok::<_, Void>((peer_id.clone(), self.muxer.clone()));
                task_ids.push(
                    handled_nodes.add_reach_attempt(fut, self.handler.clone())
                );
            }
            (handled_nodes, task_ids)
        }
    }


    // Tests for NodeTask

    #[test]
    fn task_emits_event_when_things_happen_in_the_node() {
        let (node_task, tx, mut rx) = NodeTaskTestBuilder::new()
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
        let (node_task, _tx, rx) = NodeTaskTestBuilder::new()
            .with_inner_fut(future::err(io::Error::new(io::ErrorKind::Other, "nah")))
            .with_task_id(345)
            .node_task();

        rt.spawn(node_task);
        let events = rt.block_on(rx.collect()).expect("rx failed");
        assert!(events.len() == 1);
        assert_matches!(events[0], (InToExtMessage::TaskClosed{..}, TaskId(345)));
    }

    #[test]
    fn task_exits_when_node_is_done() {
        let mut rt = Runtime::new().unwrap();
        let fut = {
            let peer_id = PeerId::random();
            let mut muxer = DummyMuxer::new();
            muxer.set_inbound_connection_state(DummyConnectionState::Closed);
            muxer.set_outbound_connection_state(DummyConnectionState::Closed);
            future::ok((peer_id, muxer))
        };
        let (node_task, tx, rx) = NodeTaskTestBuilder::new()
            .with_inner_fut(fut)
            .with_task_id(345)
            .node_task();

        // Even though we set up the muxer outbound state to be `Closed` we
        // still need to open a substream or the outbound state will never
        // be checked (see https://github.com/libp2p/rust-libp2p/issues/609).
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


    // Tests for HandledNodeTasks

    #[test]
    fn query_for_tasks() {
        let (mut handled_nodes, task_ids) = HandledNodeTaskTestBuilder::new()
            .with_tasks(3)
            .handled_nodes_tasks();

        assert_eq!(task_ids.len(), 3);
        assert_eq!(handled_nodes.task(TaskId(2)).unwrap().id(), task_ids[2]);
        assert!(handled_nodes.task(TaskId(545534)).is_none());
    }

    #[test]
    fn send_event_to_task() {
        let (mut handled_nodes, _) = HandledNodeTaskTestBuilder::new()
            .with_tasks(1)
            .handled_nodes_tasks();

        let task_id = {
            let mut task = handled_nodes.task(TaskId(0)).expect("can fetch a Task");
            task.send_event(InEvent::Custom("banana"));
            task.id()
        };

        let mut rt = Builder::new().core_threads(1).build().unwrap();
        let mut events = rt.block_on(handled_nodes.into_future()).unwrap();
        assert_matches!(events.0.unwrap(), HandledNodesEvent::NodeReached{..});

        events = rt.block_on(events.1.into_future()).unwrap();
        assert_matches!(events.0.unwrap(), HandledNodesEvent::NodeEvent{id: event_task_id, event} => {
            assert_eq!(event_task_id, task_id);
            assert_matches!(event, OutEvent::Custom("banana"));
        });
    }

    #[test]
    fn iterate_over_all_tasks() {
        let (handled_nodes, task_ids) = HandledNodeTaskTestBuilder::new()
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

        handled_nodes.add_reach_attempt( future::empty::<_, Void>(), Handler::default() );

        assert_eq!(handled_nodes.tasks().count(), 1);
        assert_eq!(handled_nodes.to_spawn.len(), 1);
    }

    #[test]
    fn running_handled_tasks_reaches_the_nodes() {
        let (mut handled_nodes_tasks, _) = HandledNodeTaskTestBuilder::new()
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

        let (mut handled_nodes_tasks, _) = HandledNodeTaskTestBuilder::new()
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

        let mut rt = Builder::new().core_threads(1).build().unwrap();
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
