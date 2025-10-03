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
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc, Mutex},
    task::{Context, Poll, Waker},
};

use crate::{types::RpcOut, MessageId};

const CONTROL_MSGS_LIMIT: usize = 20_000;

/// An async priority queue used to dispatch messages from the `NetworkBehaviour`
/// Provides a clean abstraction over high-priority (unbounded), control (bounded),
/// and non priority (bounded) message queues.
#[derive(Debug)]
pub(crate) struct Queue {
    /// High-priority unbounded queue (Subscribe, Unsubscribe)
    pub(crate) priority: Shared,
    /// Control messages bounded queue (Graft, Prune, IDontWant)
    pub(crate) control: Shared,
    /// Low-priority bounded queue (Publish, Forward, IHave, IWant)
    pub(crate) non_priority: Shared,
    /// The id of the current reference of the counter.
    pub(crate) id: usize,
    /// The total number of references for the queue.
    pub(crate) count: Arc<AtomicUsize>,
}

impl Queue {
    /// Create a new `Queue` with `capacity`.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            priority: Shared::new(),
            control: Shared::with_capacity(CONTROL_MSGS_LIMIT),
            non_priority: Shared::with_capacity(capacity),
            id: 1,
            count: Arc::new(AtomicUsize::new(1)),
        }
    }

    /// Try to push a message to the Queue, return Err if the queue is full,
    /// which will only happen for control and non priority messages.
    pub(crate) fn try_push(&mut self, message: RpcOut) -> Result<(), Box<RpcOut>> {
        match message {
            RpcOut::Subscribe(_) | RpcOut::Unsubscribe(_) => {
                self.priority
                    .try_push(message)
                    .expect("Shared is unbounded");
                Ok(())
            }
            RpcOut::Graft(_) | RpcOut::Prune(_) | RpcOut::IDontWant(_) => {
                self.control.try_push(message)
            }
            RpcOut::Publish { .. }
            | RpcOut::Forward { .. }
            | RpcOut::IHave(_)
            | RpcOut::IWant(_) => self.non_priority.try_push(message),
        }
    }

    /// Remove pending low priority Publish and Forward messages.
    /// Returns the number of messages removed.
    pub(crate) fn remove_data_messages(&mut self, message_ids: &[MessageId]) -> usize {
        let mut count = 0;
        self.non_priority.retain(|message| match message {
            RpcOut::Publish { message_id, .. } | RpcOut::Forward { message_id, .. } => {
                if message_ids.contains(message_id) {
                    count += 1;
                    false
                } else {
                    true
                }
            }
            _ => true,
        });
        count
    }

    /// Pop an element from the queue.
    pub(crate) fn poll_pop(&mut self, cx: &mut Context) -> Poll<RpcOut> {
        // First we try the priority messages.
        if let Poll::Ready(rpc) = Pin::new(&mut self.priority).poll_pop(cx) {
            return Poll::Ready(rpc);
        }

        // Then we try the control messages.
        if let Poll::Ready(rpc) = Pin::new(&mut self.control).poll_pop(cx) {
            return Poll::Ready(rpc);
        }

        // Finally we try the non priority messages
        if let Poll::Ready(rpc) = Pin::new(&mut self.non_priority).poll_pop(cx) {
            return Poll::Ready(rpc);
        }

        Poll::Pending
    }

    /// Check if the queue is empty.
    pub(crate) fn is_empty(&self) -> bool {
        if !self.priority.is_empty() {
            return false;
        }

        if !self.control.is_empty() {
            return false;
        }

        if !self.non_priority.is_empty() {
            return false;
        }

        true
    }

    /// Returns the length of the priority queue.
    #[cfg(feature = "metrics")]
    pub(crate) fn priority_len(&self) -> usize {
        self.priority.len() + self.control.len()
    }

    /// Returns the length of the non priority queue.
    #[cfg(feature = "metrics")]
    pub(crate) fn non_priority_len(&self) -> usize {
        self.non_priority.len()
    }

    /// Attempts to pop a message from the queue.
    /// returns None if the queue is empty.
    #[cfg(test)]
    pub(crate) fn try_pop(&mut self) -> Option<RpcOut> {
        // Try priority first
        self.priority
            .try_pop()
            // Then control messages
            .or_else(|| self.control.try_pop())
            // Finally non priority
            .or_else(|| self.non_priority.try_pop())
    }
}

impl Clone for Queue {
    fn clone(&self) -> Self {
        let new_id = self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self {
            priority: Shared {
                inner: self.priority.inner.clone(),
                capacity: self.priority.capacity,
                id: new_id,
            },
            control: Shared {
                inner: self.control.inner.clone(),
                capacity: self.control.capacity,
                id: new_id,
            },
            non_priority: Shared {
                inner: self.non_priority.inner.clone(),
                capacity: self.non_priority.capacity,
                id: new_id,
            },
            id: self.id,
            count: self.count.clone(),
        }
    }
}

/// The internal shared part of the queue,
/// that allows for shallow copies of the queue among each connection of the remote.
#[derive(Debug)]
pub(crate) struct Shared {
    inner: Arc<Mutex<SharedInner>>,
    capacity: Option<usize>,
    id: usize,
}

impl Shared {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SharedInner {
                queue: VecDeque::new(),
                pending_pops: Default::default(),
            })),
            capacity: Some(capacity),
            id: 1,
        }
    }

    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SharedInner {
                queue: VecDeque::new(),
                pending_pops: Default::default(),
            })),
            capacity: None,
            id: 1,
        }
    }

    /// Pop an element from the queue.
    pub(crate) fn poll_pop(self: std::pin::Pin<&mut Self>, cx: &mut Context) -> Poll<RpcOut> {
        let mut guard = self.inner.lock().expect("lock to not be poisoned");
        match guard.queue.pop_front() {
            Some(t) => Poll::Ready(t),
            None => {
                guard
                    .pending_pops
                    .entry(self.id)
                    .or_insert(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    pub(crate) fn try_push(&mut self, message: RpcOut) -> Result<(), Box<RpcOut>> {
        let mut guard = self.inner.lock().expect("lock to not be poisoned");
        if self
            .capacity
            .is_some_and(|capacity| guard.queue.len() >= capacity)
        {
            return Err(Box::new(message));
        }

        guard.queue.push_back(message);
        // Wake pending registered pops.
        for (_, s) in guard.pending_pops.drain() {
            s.wake();
        }

        Ok(())
    }

    /// Retain only the elements specified by the predicate.
    /// In other words, remove all elements e for which f(&e) returns false. The elements are
    /// visited in unsorted (and unspecified) order. Returns the cleared messages.
    pub(crate) fn retain<F: FnMut(&RpcOut) -> bool>(&mut self, f: F) {
        let mut shared = self.inner.lock().expect("lock to not be poisoned");
        shared.queue.retain(f);
    }

    /// Check if the queue is empty.
    pub(crate) fn is_empty(&self) -> bool {
        let guard = self.inner.lock().expect("lock to not be poisoned");
        guard.queue.len() == 0
    }

    /// Returns the length of the queue.
    #[cfg(feature = "metrics")]
    pub(crate) fn len(&self) -> usize {
        let guard = self.inner.lock().expect("lock to not be poisoned");
        guard.queue.len()
    }

    /// Attempts to pop an message from the queue.
    /// returns None if the queue is empty.
    #[cfg(test)]
    pub(crate) fn try_pop(&mut self) -> Option<RpcOut> {
        let mut guard = self.inner.lock().expect("lock to not be poisoned");
        guard.queue.pop_front()
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        let mut guard = self.inner.lock().expect("lock to not be poisoned");
        guard.pending_pops.remove(&self.id);
    }
}

/// The shared stated by the `NetworkBehaviour`s and the `ConnectionHandler`s.
#[derive(Debug)]
struct SharedInner {
    queue: VecDeque<RpcOut>,
    pending_pops: HashMap<usize, Waker>,
}
