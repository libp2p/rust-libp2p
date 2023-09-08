use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_timer::Delay;
use futures_util::future::BoxFuture;
use futures_util::FutureExt;

use crate::Timeout;

/// Represents a map of [Future]s.
///
/// Each future must finish within the specified time and the map never outgrows its capacity.
pub struct FuturesMap<ID, O> {
    timeout: Duration,
    capacity: usize,
    inner: HashMap<ID, (BoxFuture<'static, O>, Delay)>,
    empty_waker: Option<Waker>,
    full_waker: Option<Waker>,
}

/// Error of a future pushing
#[derive(PartialEq, Debug)]
pub enum PushError<F> {
    /// The length of the set is equal to the capacity
    BeyondCapacity(F),
    /// The set already contains the given future's ID
    ReplacedFuture(F),
}

impl<ID, O> FuturesMap<ID, O> {
    pub fn new(timeout: Duration, capacity: usize) -> Self {
        Self {
            timeout,
            capacity,
            inner: Default::default(),
            empty_waker: None,
            full_waker: None,
        }
    }
}

impl<ID, O> FuturesMap<ID, O>
where
    ID: Clone + Hash + Eq + Send + 'static,
{
    /// Push a future into the map.
    ///
    /// This method inserts the given future with defined `future_id` to the set.
    /// If the length of the map is equal to the capacity, this method returns [PushError::BeyondCapacity],
    /// that contains the passed future. In that case, the future is not inserted to the map.
    /// If a future with the given `future_id` already exists, then the old future will be replaced by a new one.
    /// In that case, the returned error [PushError::ReplacedFuture] contains the old future.
    pub fn try_push<F>(&mut self, future_id: ID, future: F) -> Result<(), PushError<BoxFuture<O>>>
    where
        F: Future<Output = O> + Send + 'static,
    {
        if self.inner.len() >= self.capacity {
            return Err(PushError::BeyondCapacity(future.boxed()));
        }

        let val = self.inner.insert(
            future_id.clone(),
            (future.boxed(), Delay::new(self.timeout)),
        );

        if let Some(waker) = self.empty_waker.take() {
            waker.wake();
        }

        match val {
            Some((old_future, _)) => Err(PushError::ReplacedFuture(old_future)),
            None => Ok(()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn poll_ready_unpin(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.inner.len() < self.capacity {
            return Poll::Ready(());
        }

        self.full_waker = Some(cx.waker().clone());

        Poll::Pending
    }

    pub fn poll_unpin(&mut self, cx: &mut Context<'_>) -> Poll<(ID, Result<O, Timeout>)> {
        let res = self
            .inner
            .iter_mut()
            .find_map(|(future_id, (future, timeout))| {
                if timeout.poll_unpin(cx).is_ready() {
                    return Some((future_id.clone(), Err(Timeout::new())));
                }

                match future.poll_unpin(cx) {
                    Poll::Ready(output) => Some((future_id.clone(), Ok(output))),
                    Poll::Pending => None,
                }
            });

        match res {
            None => {
                self.empty_waker = Some(cx.waker().clone());

                Poll::Pending
            }
            Some((future_id, future_res)) => {
                self.inner.remove(&future_id);

                if let Some(waker) = self.full_waker.take() {
                    waker.wake();
                }

                Poll::Ready((future_id, future_res))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{pending, poll_fn, ready};
    use std::pin::Pin;
    use std::time::Instant;

    use super::*;

    #[test]
    fn cannot_push_more_than_capacity_tasks() {
        let mut futures = FuturesMap::new(Duration::from_secs(10), 1);

        assert!(futures.try_push("ID_1", ready(())).is_ok());
        matches!(
            futures.try_push("ID_2", ready(())),
            Err(PushError::BeyondCapacity(_))
        );
    }

    #[test]
    fn cannot_push_the_same_id_few_times() {
        let mut futures = FuturesMap::new(Duration::from_secs(10), 5);

        assert!(futures.try_push("ID", ready(())).is_ok());
        matches!(
            futures.try_push("ID", ready(())),
            Err(PushError::ReplacedFuture(_))
        );
    }

    #[tokio::test]
    async fn futures_timeout() {
        let mut futures = FuturesMap::new(Duration::from_millis(100), 1);

        let _ = futures.try_push("ID", pending::<()>());
        Delay::new(Duration::from_millis(150)).await;
        let (_, result) = poll_fn(|cx| futures.poll_unpin(cx)).await;

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
        inner: FuturesMap<u8, ()>,
    }

    impl Task {
        fn new(future: Duration, num_futures: u32, capacity: usize) -> Self {
            Self {
                future,
                num_futures: num_futures as usize,
                num_processed: 0,
                inner: FuturesMap::new(Duration::from_secs(60), capacity),
            }
        }
    }

    impl Future for Task {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();

            while this.num_processed < this.num_futures {
                if let Poll::Ready((_, result)) = this.inner.poll_unpin(cx) {
                    if result.is_err() {
                        panic!("Timeout is great than future delay")
                    }

                    this.num_processed += 1;
                    continue;
                }

                if let Poll::Ready(()) = this.inner.poll_ready_unpin(cx) {
                    // We push the constant future's ID to prove that user can use the same ID
                    // if the future was finished
                    let maybe_future = this.inner.try_push(1u8, Delay::new(this.future));
                    assert!(maybe_future.is_ok(), "we polled for readiness");

                    continue;
                }

                return Poll::Pending;
            }

            Poll::Ready(())
        }
    }
}
