use std::future::Future;
use std::task::{ready, Context, Poll};
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::{FuturesMap, PushError, Timeout};

/// Represents a list of [Future]s.
///
/// Each future must finish within the specified time and the list never outgrows its capacity.
pub struct FuturesSet<O> {
    id: u32,
    inner: FuturesMap<u32, O>,
}

impl<O> FuturesSet<O> {
    pub fn new(timeout: Duration, capacity: usize) -> Self {
        Self {
            id: 0,
            inner: FuturesMap::new(timeout, capacity),
        }
    }
}

impl<O> FuturesSet<O> {
    /// Push a future into the list.
    ///
    /// This method adds the given future to the list.
    /// If the length of the list is equal to the capacity, this method returns a error that contains the passed future.
    /// In that case, the future is not added to the set.
    pub fn try_push<F>(&mut self, future: F) -> Result<(), BoxFuture<O>>
    where
        F: Future<Output = O> + Send + 'static,
    {
        self.id = self.id.wrapping_add(1);

        match self.inner.try_push(self.id, future) {
            Ok(()) => Ok(()),
            Err(PushError::BeyondCapacity(w)) => Err(w),
            Err(PushError::Replaced(_)) => unreachable!("we never reuse IDs"),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn poll_ready_unpin(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.inner.poll_ready_unpin(cx)
    }

    pub fn poll_unpin(&mut self, cx: &mut Context<'_>) -> Poll<Result<O, Timeout>> {
        let (_, res) = ready!(self.inner.poll_unpin(cx));

        Poll::Ready(res)
    }
}
