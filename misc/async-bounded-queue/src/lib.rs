use std::collections::VecDeque;
use std::task::{Context, Poll, Waker};

/// A bounded queue for use in asynchronous contexts.
pub struct Queue<T> {
    storage: VecDeque<T>,

    queue_full_waker: Option<Waker>,
    queue_empty_waker: Option<Waker>,
}

impl<T> Queue<T> {
    /// Construct a queue with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            storage: VecDeque::with_capacity(capacity),
            queue_full_waker: None,
            queue_empty_waker: None,
        }
    }

    /// Check if we can push an item to the queue.
    ///
    /// In case the queue is full, [`Poll::Pending`] is returned and a waker is registered that will call the current task once there is a slot in the queue.
    pub fn poll_push_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.is_full() {
            return Poll::Ready(());
        }

        self.queue_full_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Attempt to push an item to the queue.
    ///
    /// This fails in case the queue is currently full.
    /// Typically, you should call [`Queue::poll_push_ready`] before calling this.
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        if self.is_full() {
            return Err(item);
        }

        self.storage.push_back(item);

        if let Some(waker) = self.queue_empty_waker.take() {
            waker.wake();
        }

        Ok(())
    }

    /// Attempt to pop an item from the queue.
    ///
    /// This will return [`Poll::Pending`] if the queue is currently empty.
    pub fn poll_pop(&mut self, cx: &mut Context<'_>) -> Poll<T> {
        if let Some(item) = self.storage.pop_front() {
            if let Some(waker) = self.queue_full_waker.take() {
                waker.wake()
            }

            return Poll::Ready(item);
        }

        self.queue_empty_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    /// Returns how many items we can fit into the queue.
    pub fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    /// Returns how many items are currently in the queue.
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Whether the queue is currently empty.
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Whether the queue is currently at capacity, i.e. full.
    pub fn is_full(&self) -> bool {
        // Technically, `==` would be enough but better safe than sorry.
        self.storage.len() >= self.storage.capacity()
    }
}
