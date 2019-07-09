// Copyright 2019 Parity Technologies (UK) Ltd.
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
    muxing::StreamMuxer,
    nodes::{
        handled_node::{HandledNode, IntoNodeHandler, NodeHandler},
        node::{Close, Substream}
    }
};
use futures::{prelude::*, stream, sync::mpsc};
use smallvec::SmallVec;
use super::{TaskId, Error};

/// Message to transmit from the public API to a task.
#[derive(Debug)]
pub enum ToTaskMessage<T> {
    /// An event to transmit to the node handler.
    HandlerEvent(T),
    /// When received, stores the parameter inside the task and keeps it alive
    /// until we have an acknowledgment that the remote has accepted our handshake.
    TakeOver(mpsc::Sender<ToTaskMessage<T>>)
}

/// Message to transmit from a task to the public API.
#[derive(Debug)]
pub enum FromTaskMessage<T, H, E, HE, C> {
    /// A connection to a node has succeeded.
    NodeReached(C),
    /// The task closed.
    TaskClosed(Error<E, HE>, Option<H>),
    /// An event from the node.
    NodeEvent(T)
}

/// Implementation of [`Future`] that handles a single node.
pub struct Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoNodeHandler<C>,
    H::Handler: NodeHandler<Substream = Substream<M>>
{
    /// The ID of this task.
    id: TaskId,

    /// Sender to transmit messages to the outside.
    sender: mpsc::Sender<(FromTaskMessage<O, H, E, <H::Handler as NodeHandler>::Error, C>, TaskId)>,

    /// Receiver of messages from the outsize.
    receiver: stream::Fuse<mpsc::Receiver<ToTaskMessage<I>>>,

    /// Inner state of this `Task`.
    state: State<F, M, H, I, O, E, C>,

    /// Channels to keep alive for as long as we don't have an acknowledgment from the remote.
    taken_over: SmallVec<[mpsc::Sender<ToTaskMessage<I>>; 1]>
}

impl<F, M, H, I, O, E, C> Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoNodeHandler<C>,
    H::Handler: NodeHandler<Substream = Substream<M>>
{
    /// Create a new task to connect and handle some node.
    pub fn new (
        i: TaskId,
        s: mpsc::Sender<(FromTaskMessage<O, H, E, <H::Handler as NodeHandler>::Error, C>, TaskId)>,
        r: mpsc::Receiver<ToTaskMessage<I>>,
        f: F,
        h: H
    ) -> Self {
        Task {
            id: i,
            sender: s,
            receiver: r.fuse(),
            state: State::Future { future: f, handler: h, events_buffer: Vec::new() },
            taken_over: SmallVec::new()
        }
    }

    /// Create a task for an existing node we are already connected to.
    pub fn node (
        i: TaskId,
        s: mpsc::Sender<(FromTaskMessage<O, H, E, <H::Handler as NodeHandler>::Error, C>, TaskId)>,
        r: mpsc::Receiver<ToTaskMessage<I>>,
        n: HandledNode<M, H::Handler>
    ) -> Self {
        Task {
            id: i,
            sender: s,
            receiver: r.fuse(),
            state: State::Node(n),
            taken_over: SmallVec::new()
        }
    }
}

/// State of the future.
enum State<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoNodeHandler<C>,
    H::Handler: NodeHandler<Substream = Substream<M>>
{
    /// Future to resolve to connect to the node.
    Future {
        /// The future that will attempt to reach the node.
        future: F,
        /// The handler that will be used to build the `HandledNode`.
        handler: H,
        /// While we are dialing the future, we need to buffer the events received on
        /// `receiver` so that they get delivered once dialing succeeds. We can't simply leave
        /// events in `receiver` because we have to detect if it gets closed.
        events_buffer: Vec<I>
    },

    /// An event should be sent to the outside world.
    SendEvent {
        /// The node, if available.
        node: Option<HandledNode<M, H::Handler>>,
        /// The actual event message to send.
        event: FromTaskMessage<O, H, E, <H::Handler as NodeHandler>::Error, C>
    },

    /// We started sending an event, now drive the sending to completion.
    ///
    /// The `bool` parameter determines if we transition to `State::Node`
    /// afterwards or to `State::Closing` (assuming we have `Some` node,
    /// otherwise the task will end).
    PollComplete(Option<HandledNode<M, H::Handler>>, bool),

    /// Fully functional node.
    Node(HandledNode<M, H::Handler>),

    /// Node closing.
    Closing(Close<M>),

    /// Interim state that can only be observed externally if the future
    /// resolved to a value previously.
    Undefined
}

impl<F, M, H, I, O, E, C> Future for Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    F: Future<Item = (C, M), Error = E>,
    H: IntoNodeHandler<C>,
    H::Handler: NodeHandler<Substream = Substream<M>, InEvent = I, OutEvent = O>
{
    type Item = ();
    type Error = ();

    // NOTE: It is imperative to always consume all incoming event messages
    // first in order to not prevent the outside from making progress because
    // they are blocked on the channel capacity.
    fn poll(&mut self) -> Poll<(), ()> {
        'poll: loop {
            match std::mem::replace(&mut self.state, State::Undefined) {
                State::Future { mut future, handler, mut events_buffer } => {
                    // If self.receiver is closed, we stop the task.
                    loop {
                        match self.receiver.poll() {
                            Ok(Async::NotReady) => break,
                            Ok(Async::Ready(None)) => return Ok(Async::Ready(())),
                            Ok(Async::Ready(Some(ToTaskMessage::HandlerEvent(event)))) =>
                                events_buffer.push(event),
                            Ok(Async::Ready(Some(ToTaskMessage::TakeOver(take_over)))) =>
                                self.taken_over.push(take_over),
                            Err(()) => unreachable!("An `mpsc::Receiver` does not error.")
                        }
                    }
                    // Check if dialing succeeded.
                    match future.poll() {
                        Ok(Async::Ready((conn_info, muxer))) => {
                            let mut node = HandledNode::new(muxer, handler.into_handler(&conn_info));
                            for event in events_buffer {
                                node.inject_event(event)
                            }
                            self.state = State::SendEvent {
                                node: Some(node),
                                event: FromTaskMessage::NodeReached(conn_info)
                            }
                        }
                        Ok(Async::NotReady) => {
                            self.state = State::Future { future, handler, events_buffer };
                            return Ok(Async::NotReady)
                        }
                        Err(e) => {
                            let event = FromTaskMessage::TaskClosed(Error::Reach(e), Some(handler));
                            self.state = State::SendEvent { node: None, event }
                        }
                    }
                }
                State::Node(mut node) => {
                    // Start by handling commands received from the outside of the task.
                    loop {
                        match self.receiver.poll() {
                            Ok(Async::NotReady) => break,
                            Ok(Async::Ready(Some(ToTaskMessage::HandlerEvent(event)))) =>
                                node.inject_event(event),
                            Ok(Async::Ready(Some(ToTaskMessage::TakeOver(take_over)))) =>
                                self.taken_over.push(take_over),
                            Ok(Async::Ready(None)) => {
                                // Node closed by the external API; start closing.
                                self.state = State::Closing(node.close());
                                continue 'poll
                            }
                            Err(()) => unreachable!("An `mpsc::Receiver` does not error.")
                        }
                    }
                    // Process the node.
                    loop {
                        if !self.taken_over.is_empty() && node.is_remote_acknowledged() {
                            self.taken_over.clear()
                        }
                        match node.poll() {
                            Ok(Async::NotReady) => {
                                self.state = State::Node(node);
                                return Ok(Async::NotReady)
                            }
                            Ok(Async::Ready(event)) => {
                                self.state = State::SendEvent {
                                    node: Some(node),
                                    event: FromTaskMessage::NodeEvent(event)
                                };
                                continue 'poll
                            }
                            Err(err) => {
                                let event = FromTaskMessage::TaskClosed(Error::Node(err), None);
                                self.state = State::SendEvent { node: None, event };
                                continue 'poll
                            }
                        }
                    }
                }
                // Deliver an event to the outside.
                State::SendEvent { mut node, event } => {
                    loop {
                        match self.receiver.poll() {
                            Ok(Async::NotReady) => break,
                            Ok(Async::Ready(Some(ToTaskMessage::HandlerEvent(event)))) =>
                                if let Some(ref mut n) = node {
                                    n.inject_event(event)
                                }
                            Ok(Async::Ready(Some(ToTaskMessage::TakeOver(take_over)))) =>
                                self.taken_over.push(take_over),
                            Ok(Async::Ready(None)) =>
                                // Node closed by the external API; start closing.
                                if let Some(n) = node {
                                    self.state = State::Closing(n.close());
                                    continue 'poll
                                } else {
                                    return Ok(Async::Ready(())) // end task
                                }
                            Err(()) => unreachable!("An `mpsc::Receiver` does not error.")
                        }
                    }
                    // Check if this task is about to close. We pass the flag to
                    // the next state so it knows what to do.
                    let close =
                        if let FromTaskMessage::TaskClosed(..) = event {
                            true
                        } else {
                            false
                        };
                    match self.sender.start_send((event, self.id)) {
                        Ok(AsyncSink::NotReady((event, _))) => {
                            self.state = State::SendEvent { node, event };
                            return Ok(Async::NotReady)
                        }
                        Ok(AsyncSink::Ready) => self.state = State::PollComplete(node, close),
                        Err(_) => {
                            if let Some(n) = node {
                                self.state = State::Closing(n.close());
                                continue 'poll
                            }
                            // We can not communicate to the outside and there is no
                            // node to handle, so this is the end of this task.
                            return Ok(Async::Ready(()))
                        }
                    }
                }
                // We started delivering an event, now try to complete the sending.
                State::PollComplete(mut node, close) => {
                    loop {
                        match self.receiver.poll() {
                            Ok(Async::NotReady) => break,
                            Ok(Async::Ready(Some(ToTaskMessage::HandlerEvent(event)))) =>
                                if let Some(ref mut n) = node {
                                    n.inject_event(event)
                                }
                            Ok(Async::Ready(Some(ToTaskMessage::TakeOver(take_over)))) =>
                                self.taken_over.push(take_over),
                            Ok(Async::Ready(None)) =>
                                // Node closed by the external API; start closing.
                                if let Some(n) = node {
                                    self.state = State::Closing(n.close());
                                    continue 'poll
                                } else {
                                    return Ok(Async::Ready(())) // end task
                                }
                            Err(()) => unreachable!("An `mpsc::Receiver` does not error.")
                        }
                    }
                    match self.sender.poll_complete() {
                        Ok(Async::NotReady) => {
                            self.state = State::PollComplete(node, close);
                            return Ok(Async::NotReady)
                        }
                        Ok(Async::Ready(())) =>
                            if let Some(n) = node {
                                if close {
                                    self.state = State::Closing(n.close())
                                } else {
                                    self.state = State::Node(n)
                                }
                            } else {
                                // Since we have no node we terminate this task.
                                assert!(close);
                                return Ok(Async::Ready(()))
                            }
                        Err(_) => {
                            if let Some(n) = node {
                                self.state = State::Closing(n.close());
                                continue 'poll
                            }
                            // We can not communicate to the outside and there is no
                            // node to handle, so this is the end of this task.
                            return Ok(Async::Ready(()))
                        }
                    }
                }
                State::Closing(mut closing) =>
                    match closing.poll() {
                        Ok(Async::Ready(())) | Err(_) =>
                            return Ok(Async::Ready(())), // end task
                        Ok(Async::NotReady) => {
                            self.state = State::Closing(closing);
                            return Ok(Async::NotReady)
                        }
                    }
                // This happens if a previous poll has resolved the future.
                // The API contract of futures is that we should not be polled again.
                State::Undefined => panic!("`Task::poll()` called after completion.")
            }
        }
    }
}

