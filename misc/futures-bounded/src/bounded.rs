use std::future::Future;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::{PushError, Timeout, UniqueWorkers};

/// Represents a set of (Worker)-[Future]s.
///
/// This wraps [FuturesUnordered] but bounds it by time and size.
/// In other words, each worker must finish within the specified time and the set never outgrows its capacity.
pub struct BoundedWorkers<O> {
    id: i32,
    inner: UniqueWorkers<i32, O>,
}

impl<O> BoundedWorkers<O> {
    pub fn new(timeout: Duration, capacity: usize) -> Self {
        Self {
            id: i32::MIN,
            inner: UniqueWorkers::new(timeout, capacity),
        }
    }
}

impl<O> BoundedWorkers<O> {
    /// Push a worker into the set.
    /// This method adds the given worker to the set.
    /// If the length of the set is equal to the capacity,
    /// this method returns a error that contains the passed worker.
    /// In that case, the worker is not added to the set.
    pub fn try_push<F>(&mut self, worker: F) -> Result<(), BoxFuture<O>>
    where
        F: Future<Output = O> + Send + 'static + Unpin,
    {
        (self.id, _) = self.id.overflowing_add(1);

        match self.inner.try_push(self.id, worker) {
            Ok(()) => Ok(()),
            Err(PushError::BeyondCapacity(w)) => Err(w),
            Err(PushError::ReplacedWorker(_)) => {
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
        match self.inner.poll_unpin(cx) {
            Poll::Ready((_, res)) => Poll::Ready(res),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, poll_fn, ready};
    use std::pin::Pin;
    use std::time::Instant;
    use futures_timer::Delay;

    use super::*;

    #[test]
    fn cannot_push_more_than_capacity_tasks() {
        let mut workers = BoundedWorkers::new(Duration::from_secs(10), 1);

        assert!(workers.try_push(ready(())).is_ok());
        assert!(workers.try_push(ready(())).is_err());
    }

    #[tokio::test]
    async fn workers_timeout() {
        let mut workers = BoundedWorkers::new(Duration::from_millis(100), 1);

        let _ = workers.try_push(pending::<()>());
        Delay::new(Duration::from_millis(150)).await;
        let result = poll_fn(|cx| workers.poll_unpin(cx)).await;

        assert!(result.is_err())
    }

    // Each worker causes a delay, `Task` only has a capacity of 1, meaning they must be processed in sequence.
    // We stop after NUM_WORKERS tasks, meaning the overall execution must at least take DELAY * NUM_WORKERS.
    #[tokio::test]
    async fn backpressure() {
        const DELAY: Duration = Duration::from_millis(100);
        const NUM_WORKERS: u32 = 10;

        let start = Instant::now();
        Task::new(DELAY, NUM_WORKERS, 1).await;
        let duration = start.elapsed();

        assert!(duration >= DELAY * NUM_WORKERS);
    }

    struct Task {
        worker: Duration,
        num_workers: usize,
        num_processed: usize,
        inner: BoundedWorkers<()>,
    }

    impl Task {
        fn new(worker: Duration, num_workers: u32, capacity: usize) -> Self {
            Self {
                worker,
                num_workers: num_workers as usize,
                num_processed: 0,
                inner: BoundedWorkers::new(Duration::from_secs(60), capacity),
            }
        }
    }

    impl Future for Task {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();

            while this.num_processed < this.num_workers {
                if let Poll::Ready(result) = this.inner.poll_unpin(cx) {
                    if result.is_err() {
                        panic!("Timeout is great than worker delay")
                    }

                    this.num_processed += 1;
                    continue;
                }

                if let Poll::Ready(()) = this.inner.poll_ready_unpin(cx) {
                    let maybe_worker = this.inner.try_push(Delay::new(this.worker));
                    assert!(maybe_worker.is_ok(), "we polled for readiness");

                    continue;
                }

                return Poll::Pending;
            }

            Poll::Ready(())
        }
    }
}
