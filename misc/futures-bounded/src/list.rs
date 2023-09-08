use std::future::Future;
use std::task::{ready, Context, Poll};
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::{FuturesMap, PushError, Timeout};

/// Represents a list of [Future]s.
///
/// Each future must finish within the specified time and the list never outgrows its capacity.
pub struct FuturesList<O> {
    id: i32,
    inner: FuturesMap<i32, O>,
}

impl<O> FuturesList<O> {
    pub fn new(timeout: Duration, capacity: usize) -> Self {
        Self {
            id: i32::MIN,
            inner: FuturesMap::new(timeout, capacity),
        }
    }
}

impl<O> FuturesList<O> {
    /// Push a future into the list.
    /// This method adds the given future to the list.
    /// If the length of the list is equal to the capacity,
    /// this method returns a error that contains the passed future.
    /// In that case, the future is not added to the set.
    pub fn try_push<F>(&mut self, future: F) -> Result<(), BoxFuture<O>>
    where
        F: Future<Output = O> + Send + 'static,
    {
        (self.id, _) = self.id.overflowing_add(1);

        match self.inner.try_push(self.id, future) {
            Ok(()) => Ok(()),
            Err(PushError::BeyondCapacity(w)) => Err(w),
            Err(PushError::ReplacedFuture(_)) => {
                unreachable!()
            }
        }
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

#[cfg(test)]
mod tests {
    use futures_timer::Delay;
    use std::future::{pending, poll_fn, ready};
    use std::pin::Pin;
    use std::time::Instant;

    use super::*;

    #[test]
    fn cannot_push_more_than_capacity_tasks() {
        let mut futures = FuturesList::new(Duration::from_secs(10), 1);

        assert!(futures.try_push(ready(())).is_ok());
        assert!(futures.try_push(ready(())).is_err());
    }

    #[tokio::test]
    async fn futures_timeout() {
        let mut futures = FuturesList::new(Duration::from_millis(100), 1);

        let _ = futures.try_push(pending::<()>());
        Delay::new(Duration::from_millis(150)).await;
        let result = poll_fn(|cx| futures.poll_unpin(cx)).await;

        assert!(result.is_err())
    }

    // Each future causes a delay, `Task` only has a capacity of 1, meaning they must be processed in sequence.
    // We stop after NUM_FUTURES tasks, meaning the overall execution must at least take DELAY * NUM_FUTURES.
    #[tokio::test]
    async fn backpressure() {
        const DELAY: Duration = Duration::from_millis(100);
        const NUM_FUTURES: u32 = 10;

        let start = Instant::now();
        Task::new(DELAY, NUM_FUTURES, 1).await;
        let duration = start.elapsed();

        assert!(duration >= DELAY * NUM_FUTURES);
    }

    struct Task {
        future: Duration,
        num_futures: usize,
        num_processed: usize,
        inner: FuturesList<()>,
    }

    impl Task {
        fn new(future: Duration, num_futures: u32, capacity: usize) -> Self {
            Self {
                future,
                num_futures: num_futures as usize,
                num_processed: 0,
                inner: FuturesList::new(Duration::from_secs(60), capacity),
            }
        }
    }

    impl Future for Task {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();

            while this.num_processed < this.num_futures {
                if let Poll::Ready(result) = this.inner.poll_unpin(cx) {
                    if result.is_err() {
                        panic!("Timeout is great than future delay")
                    }

                    this.num_processed += 1;
                    continue;
                }

                if let Poll::Ready(()) = this.inner.poll_ready_unpin(cx) {
                    let maybe_future = this.inner.try_push(Delay::new(this.future));
                    assert!(maybe_future.is_ok(), "we polled for readiness");

                    continue;
                }

                return Poll::Pending;
            }

            Poll::Ready(())
        }
    }
}
