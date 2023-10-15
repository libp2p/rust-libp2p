use futures_util::stream::BoxStream;
use futures_util::Stream;
use std::task::{ready, Context, Poll};
use std::time::Duration;

use crate::{PushError, StreamMap, Timeout};

/// Represents a list of [Stream]s.
///
/// Each stream must finish within the specified time and the list never outgrows its capacity.
pub struct StreamSet<O> {
    id: u32,
    inner: StreamMap<u32, O>,
}

impl<O> StreamSet<O> {
    pub fn new(timeout: Duration, capacity: usize) -> Self {
        Self {
            id: 0,
            inner: StreamMap::new(timeout, capacity),
        }
    }
}

impl<O> StreamSet<O>
where
    O: Send + 'static,
{
    /// Push a stream into the list.
    ///
    /// This method adds the given stream to the list.
    /// If the length of the list is equal to the capacity, this method returns a error that contains the passed stream.
    /// In that case, the stream is not added to the set.
    pub fn try_push<F>(&mut self, stream: F) -> Result<(), BoxStream<O>>
    where
        F: Stream<Item = O> + Send + 'static,
    {
        self.id = self.id.wrapping_add(1);

        match self.inner.try_push(self.id, stream) {
            Ok(()) => Ok(()),
            Err(PushError::BeyondCapacity(w)) => Err(w),
            Err(PushError::Replaced(_)) => unreachable!("we never reuse IDs"),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn poll_ready_unpin(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.inner.poll_ready_unpin(cx)
    }

    pub fn poll_next_unpin(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<O, Timeout>>> {
        let (_, res) = ready!(self.inner.poll_next_unpin(cx));

        Poll::Ready(res)
    }
}
