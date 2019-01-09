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

#![cfg(test)]

use super::*;

use std::io;

use assert_matches::assert_matches;
use futures::future::{self, FutureResult};
use futures::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use crate::nodes::handled_node::NodeHandlerEvent;
use crate::tests::dummy_handler::{Handler, HandlerState, InEvent, OutEvent, TestHandledNode};
use crate::tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
use tokio::runtime::Builder;
use tokio::runtime::current_thread::Runtime;
use void::Void;
use crate::PeerId;

type TestNodeTask = NodeTask<
    FutureResult<(PeerId, DummyMuxer), io::Error>,
    DummyMuxer,
    Handler,
    InEvent,
    OutEvent,
    io::Error,
>;

struct NodeTaskTestBuilder {
    task_id: TaskId,
    inner_node: Option<TestHandledNode>,
    inner_fut: Option<FutureResult<(PeerId, DummyMuxer), io::Error>>,
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

    fn with_inner_fut(&mut self, fut: FutureResult<(PeerId, DummyMuxer), io::Error>) -> &mut Self{
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
        UnboundedReceiver<(InToExtMessage<OutEvent, Handler, io::Error, io::Error>, TaskId)>,
    ) {
        let (events_from_node_task_tx, events_from_node_task_rx) = mpsc::unbounded::<(InToExtMessage<OutEvent, Handler, _, _>, TaskId)>();
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

type TestHandledNodesTasks = HandledNodesTasks<InEvent, OutEvent, Handler, io::Error, io::Error>;

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
            let fut = future::ok((peer_id.clone(), self.muxer.clone()));
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
    // Async::Ready(()). QED.

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
    let mut events: (Option<HandledNodesEvent<_, _, _, _>>, TestHandledNodesTasks);
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
