// Copyright 2020 Sigma Prime Pty Ltd.
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

use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use futures::{stream::Peekable, Stream, StreamExt};

use crate::types::RpcOut;

/// `RpcOut` sender that is priority aware.
#[derive(Debug)]
pub(crate) struct Sender {
    /// Capacity of the priority channel for `Publish` messages.
    priority_cap: usize,
    len: Arc<AtomicUsize>,
    pub(crate) priority_sender: async_channel::Sender<RpcOut>,
    pub(crate) non_priority_sender: async_channel::Sender<RpcOut>,
    priority_receiver: async_channel::Receiver<RpcOut>,
    non_priority_receiver: async_channel::Receiver<RpcOut>,
}

impl Sender {
    /// Create a RpcSender.
    pub(crate) fn new(cap: usize) -> Sender {
        // We intentionally do not bound the channel, as we still need to send control messages
        // such as `GRAFT`, `PRUNE`, `SUBSCRIBE`, and `UNSUBSCRIBE`.
        // That's also why we define `cap` and divide it by two,
        // to ensure there is capacity for both priority and non_priority messages.
        let (priority_sender, priority_receiver) = async_channel::unbounded();
        let (non_priority_sender, non_priority_receiver) = async_channel::bounded(cap / 2);
        let len = Arc::new(AtomicUsize::new(0));
        Sender {
            priority_cap: cap / 2,
            len,
            priority_sender,
            non_priority_sender,
            priority_receiver,
            non_priority_receiver,
        }
    }

    /// Create a new Receiver to the sender.
    pub(crate) fn new_receiver(&self) -> Receiver {
        Receiver {
            priority_queue_len: self.len.clone(),
            priority: Box::pin(self.priority_receiver.clone().peekable()),
            non_priority: Box::pin(self.non_priority_receiver.clone().peekable()),
        }
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn send_message(&self, rpc: RpcOut) -> Result<(), RpcOut> {
        if let RpcOut::Publish { .. } = rpc {
            // Update number of publish message in queue.
            let len = self.len.load(Ordering::Relaxed);
            if len >= self.priority_cap {
                return Err(rpc);
            }
            self.len.store(len + 1, Ordering::Relaxed);
        }
        let sender = match rpc {
            RpcOut::Publish { .. }
            | RpcOut::Graft(_)
            | RpcOut::Prune(_)
            | RpcOut::Subscribe(_)
            | RpcOut::Unsubscribe(_) => &self.priority_sender,
            RpcOut::Forward { .. } | RpcOut::IHave(_) | RpcOut::IWant(_) => {
                &self.non_priority_sender
            }
        };
        sender.try_send(rpc).map_err(|err| err.into_inner())
    }

    /// Returns the current size of the priority queue.
    pub(crate) fn priority_queue_len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    /// Returns the current size of the non-priority queue.
    pub(crate) fn non_priority_queue_len(&self) -> usize {
        self.non_priority_sender.len()
    }
}

/// `RpcOut` sender that is priority aware.
#[derive(Debug)]
pub struct Receiver {
    /// The maximum length of the priority queue.
    pub(crate) priority_queue_len: Arc<AtomicUsize>,
    /// The priority queue receiver.
    pub(crate) priority: Pin<Box<Peekable<async_channel::Receiver<RpcOut>>>>,
    /// The non priority queue receiver.
    pub(crate) non_priority: Pin<Box<Peekable<async_channel::Receiver<RpcOut>>>>,
}

impl Receiver {
    // Peek the next message in the queues and return it if its timeout has elapsed.
    // Returns `None` if there aren't any more messages on the stream or none is stale.
    pub(crate) fn poll_stale(&mut self, cx: &mut Context<'_>) -> Poll<Option<RpcOut>> {
        // Peek priority queue.
        let priority = match self.priority.as_mut().poll_peek_mut(cx) {
            Poll::Ready(Some(RpcOut::Publish {
                message: _,
                ref mut timeout,
            })) => {
                if Pin::new(timeout).poll(cx).is_ready() {
                    // Return the message.
                    let dropped = futures::ready!(self.priority.poll_next_unpin(cx))
                        .expect("There should be a message");
                    return Poll::Ready(Some(dropped));
                }
                Poll::Ready(None)
            }
            poll => poll,
        };

        let non_priority = match self.non_priority.as_mut().poll_peek_mut(cx) {
            Poll::Ready(Some(RpcOut::Forward {
                message: _,
                ref mut timeout,
            })) => {
                if Pin::new(timeout).poll(cx).is_ready() {
                    // Return the message.
                    let dropped = futures::ready!(self.non_priority.poll_next_unpin(cx))
                        .expect("There should be a message");
                    return Poll::Ready(Some(dropped));
                }
                Poll::Ready(None)
            }
            poll => poll,
        };

        match (priority, non_priority) {
            (Poll::Ready(None), Poll::Ready(None)) => Poll::Ready(None),
            _ => Poll::Pending,
        }
    }

    /// Poll queues and return true if both are empty.
    pub(crate) fn poll_is_empty(&mut self, cx: &mut Context<'_>) -> bool {
        matches!(
            (
                self.priority.as_mut().poll_peek(cx),
                self.non_priority.as_mut().poll_peek(cx),
            ),
            (Poll::Ready(None), Poll::Ready(None))
        )
    }
}

impl Stream for Receiver {
    type Item = RpcOut;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // The priority queue is first polled.
        if let Poll::Ready(rpc) = Pin::new(&mut self.priority).poll_next(cx) {
            if let Some(RpcOut::Publish { .. }) = rpc {
                self.priority_queue_len.fetch_sub(1, Ordering::Relaxed);
            }
            return Poll::Ready(rpc);
        }
        // Then we poll the non priority.
        Pin::new(&mut self.non_priority).poll_next(cx)
    }
}
