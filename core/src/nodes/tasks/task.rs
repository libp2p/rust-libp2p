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
use futures::{prelude::*, channel::mpsc, stream};
use smallvec::SmallVec;
use std::{mem, pin::Pin, task::Context, task::Poll};
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
            state: State::Future { future: Box::pin(f), handler: h, events_buffer: Vec::new() },
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
        // TODO: don't pin this Future; this requires deeper changes though
        future: Pin<Box<F>>,
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

    /// Fully functional node.
    Node(HandledNode<M, H::Handler>),

    /// Node closing.
    Closing(Close<M>),

    /// Interim state that can only be observed externally if the future
    /// resolved to a value previously.
    Undefined
}

impl<F, M, H, I, O, E, C> Unpin for Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoNodeHandler<C>,
    H::Handler: NodeHandler<Substream = Substream<M>>
{
}

impl<F, M, H, I, O, E, C> Future for Task<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    F: Future<Output = Result<(C, M), E>>,
    H: IntoNodeHandler<C>,
    H::Handler: NodeHandler<Substream = Substream<M>, InEvent = I, OutEvent = O>
{
    type Output = ();

    // NOTE: It is imperative to always consume all incoming event messages
    // first in order to not prevent the outside from making progress because
    // they are blocked on the channel capacity.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        // We use a `this` because the compiler isn't smart enough to allow mutably borrowing
        // multiple different fields from the `Pin` at the same time.
        let this = &mut *self;

        'poll: loop {
            match &mut this.state {
                State::Future { future, events_buffer, .. } => {
                    // If this.receiver is closed, we stop the task.
                    loop {
                        match Stream::poll_next(Pin::new(&mut this.receiver), cx) {
                            Poll::Pending => break,
                            Poll::Ready(None) => {
                                this.state = State::Undefined;
                                return Poll::Ready(())
                            },
                            Poll::Ready(Some(ToTaskMessage::HandlerEvent(event))) =>
                                events_buffer.push(event),
                            Poll::Ready(Some(ToTaskMessage::TakeOver(take_over))) =>
                                this.taken_over.push(take_over),
                        }
                    }
                    // Check if dialing succeeded.
                    match Future::poll(Pin::new(future), cx) {
                        Poll::Ready(Ok((conn_info, muxer))) => {
                            let (handler, events_buffer) = this.state.extract_handler_events_buffer()
                                .expect("can only be reached if this.state is Future; qed");
                            let mut node = HandledNode::new(muxer, handler.into_handler(&conn_info));
                            for event in events_buffer {
                                node.inject_event(event)
                            }
                            this.state = State::SendEvent {
                                node: Some(node),
                                event: FromTaskMessage::NodeReached(conn_info)
                            }
                        }
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(e)) => {
                            let (handler, _) = this.state.extract_handler_events_buffer()
                                .expect("can only be reached if this.state is Future; qed");
                            let event = FromTaskMessage::TaskClosed(Error::Reach(e), Some(handler));
                            this.state = State::SendEvent { node: None, event }
                        }
                    }
                }
                State::Node(node) => {
                    // Start by handling commands received from the outside of the task.
                    loop {
                        match Stream::poll_next(Pin::new(&mut this.receiver), cx) {
                            Poll::Pending => break,
                            Poll::Ready(Some(ToTaskMessage::HandlerEvent(event))) =>
                                node.inject_event(event),
                            Poll::Ready(Some(ToTaskMessage::TakeOver(take_over))) =>
                                this.taken_over.push(take_over),
                            Poll::Ready(None) => {
                                // Node closed by the external API; start closing.
                                let node = this.state.extract_node()
                                    .expect("can only be reached if this.state is Node; qed");
                                this.state = State::Closing(node.close());
                                continue 'poll
                            }
                        }
                    }
                    // Process the node.
                    loop {
                        if !this.taken_over.is_empty() && node.is_remote_acknowledged() {
                            this.taken_over.clear()
                        }
                        match HandledNode::poll(Pin::new(node), cx) {
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(Ok(event)) => {
                                let node = this.state.extract_node()
                                    .expect("can only be reached if this.state is Node; qed");
                                this.state = State::SendEvent {
                                    node: Some(node),
                                    event: FromTaskMessage::NodeEvent(event)
                                };
                                continue 'poll
                            }
                            Poll::Ready(Err(err)) => {
                                let event = FromTaskMessage::TaskClosed(Error::Node(err), None);
                                this.state = State::SendEvent { node: None, event };
                                continue 'poll
                            }
                        }
                    }
                }
                // Deliver an event to the outside.
                State::SendEvent { node, event } => {
                    loop {
                        match Stream::poll_next(Pin::new(&mut this.receiver), cx) {
                            Poll::Pending => break,
                            Poll::Ready(Some(ToTaskMessage::HandlerEvent(event))) =>
                                if let Some(ref mut n) = node {
                                    n.inject_event(event)
                                }
                            Poll::Ready(Some(ToTaskMessage::TakeOver(take_over))) =>
                                this.taken_over.push(take_over),
                            Poll::Ready(None) =>
                                // Node closed by the external API; start closing.
                                if node.is_some() {
                                    let node = this.state.extract_node()
                                        .expect("we just checked node.is_some(); qed");
                                    this.state = State::Closing(node.close());
                                    continue 'poll
                                } else {
                                    this.state = State::Undefined;
                                    return Poll::Ready(()) // end task
                                }
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
                    match this.sender.poll_ready(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Ok(())) => {
                            // We assume that if `poll_ready` has succeeded, then sending the event
                            // will succeed as well. If it turns out that it didn't, we will detect
                            // the closing at the next loop iteration.
                            let (node, event) = this.state.extract_send_event()
                                .expect("we can only reach here from SendEvent; qed");
                            let _ = this.sender.start_send((event, this.id));
                            if let Some(n) = node {
                                if close {
                                    this.state = State::Closing(n.close())
                                } else {
                                    this.state = State::Node(n)
                                }
                            } else {
                                // Since we have no node we terminate this task.
                                assert!(close);
                                return Poll::Ready(())
                            }
                        },
                        Poll::Ready(Err(_)) => {
                            let (node, _) = this.state.extract_send_event()
                                .expect("we can only reach here from SendEvent; qed");
                            if let Some(node) = node {
                                this.state = State::Closing(node.close());
                                continue 'poll
                            }
                            // We can not communicate to the outside and there is no
                            // node to handle, so this is the end of this task.
                            return Poll::Ready(())
                        }
                    }
                }
                State::Closing(closing) =>
                    match Future::poll(Pin::new(closing), cx) {
                        Poll::Ready(_) => {
                            this.state = State::Undefined;
                            return Poll::Ready(()) // end task
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                // This happens if a previous poll has resolved the future.
                // The API contract of futures is that we should not be polled again.
                State::Undefined => panic!("`Task::poll()` called after completion.")
            }
        }
    }
}

impl<F, M, H, I, O, E, C> State<F, M, H, I, O, E, C>
where
    M: StreamMuxer,
    H: IntoNodeHandler<C>,
    H::Handler: NodeHandler<Substream = Substream<M>>
{
    /// Extracts the `NodeHandler` from `self`, if any.
    /// If so, replaces `self` with `Undefined`.
    fn extract_node(&mut self) -> Option<HandledNode<M, H::Handler>> {
        match self {
            State::SendEvent { node: Some(_), .. } => {
                if let State::SendEvent { node: Some(node), .. } =
                    mem::replace(self, State::Undefined)
                {
                    return Some(node);
                } else {
                    unreachable!("we just checked the same match pattern above; qed")
                }
            }
            State::Node(_) => {
                if let State::Node(node) = mem::replace(self, State::Undefined) {
                    return Some(node);
                } else {
                    unreachable!("we just checked the same match pattern above; qed")
                }
            }
            _ => None,
        }
    }

    /// Extracts the components of `Future` from `self`, if relevant.
    /// If so, replaces `self` with `Undefined`.
    fn extract_handler_events_buffer(&mut self) -> Option<(H, Vec<I>)> {
        match self {
            State::Future { .. } => {
                if let State::Future { handler, events_buffer, .. } =
                    mem::replace(self, State::Undefined)
                {
                    return Some((handler, events_buffer));
                } else {
                    unreachable!("we just checked the same match pattern above; qed")
                }
            }
            _ => None,
        }
    }

    /// Extracts the components of `SendEvent` from `self`, if relevant.
    /// If so, replaces `self` with `Undefined`.
    fn extract_send_event(&mut self)
        -> Option<(Option<HandledNode<M, H::Handler>>, FromTaskMessage<O, H, E, <H::Handler as NodeHandler>::Error, C>)>
    {
        match self {
            State::SendEvent { .. } => {
                if let State::SendEvent { node, event } =
                    mem::replace(self, State::Undefined)
                {
                    return Some((node, event));
                } else {
                    unreachable!("we just checked the same match pattern above; qed")
                }
            }
            _ => None,
        }
    }
}
