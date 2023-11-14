use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use std::{future, mem};

use futures_timer::Delay;
use futures_util::future::BoxFuture;
use futures_util::stream::FuturesUnordered;
use futures_util::{FutureExt, StreamExt};

use crate::{PushError, Timeout};

/// Represents a map of [`Future`]s.
///
/// Each future must finish within the specified time and the map never outgrows its capacity.
pub struct FuturesMap<ID, O> {
    timeout: Duration,
    capacity: usize,
    inner: FuturesUnordered<TaggedFuture<ID, TimeoutFuture<BoxFuture<'static, O>>>>,
    empty_waker: Option<Waker>,
    full_waker: Option<Waker>,
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
    ID: Clone + Hash + Eq + Send + Unpin + 'static,
    O: 'static,
{
    /// Push a future into the map.
    ///
    /// This method inserts the given future with defined `future_id` to the set.
    /// If the length of the map is equal to the capacity, this method returns [PushError::BeyondCapacity],
    /// that contains the passed future. In that case, the future is not inserted to the map.
    /// If a future with the given `future_id` already exists, then the old future will be replaced by a new one.
    /// In that case, the returned error [PushError::Replaced] contains the old future.
    pub fn try_push<F>(&mut self, future_id: ID, future: F) -> Result<(), PushError<BoxFuture<O>>>
    where
        F: Future<Output = O> + Send + 'static,
    {
        if self.inner.len() >= self.capacity {
            return Err(PushError::BeyondCapacity(future.boxed()));
        }

        if let Some(waker) = self.empty_waker.take() {
            waker.wake();
        }

        let old = self.remove(future_id.clone());
        self.inner.push(TaggedFuture {
            tag: future_id,
            inner: TimeoutFuture {
                inner: future.boxed(),
                timeout: Delay::new(self.timeout),
                cancelled: false,
            },
        });
        match old {
            None => Ok(()),
            Some(old) => Err(PushError::Replaced(old)),
        }
    }

    pub fn remove(&mut self, id: ID) -> Option<BoxFuture<'static, O>> {
        let tagged = self.inner.iter_mut().find(|s| s.tag == id)?;

        let inner = mem::replace(&mut tagged.inner.inner, future::pending().boxed());
        tagged.inner.cancelled = true;

        Some(inner)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[allow(unknown_lints, clippy::needless_pass_by_ref_mut)] // &mut Context is idiomatic.
    pub fn poll_ready_unpin(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.inner.len() < self.capacity {
            return Poll::Ready(());
        }

        self.full_waker = Some(cx.waker().clone());

        Poll::Pending
    }

    pub fn poll_unpin(&mut self, cx: &mut Context<'_>) -> Poll<(ID, Result<O, Timeout>)> {
        loop {
            let maybe_result = futures_util::ready!(self.inner.poll_next_unpin(cx));

            match maybe_result {
                None => {
                    self.empty_waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }
                Some((id, Ok(output))) => return Poll::Ready((id, Ok(output))),
                Some((id, Err(TimeoutError::Timeout))) => {
                    return Poll::Ready((id, Err(Timeout::new(self.timeout))))
                }
                Some((_, Err(TimeoutError::Cancelled))) => continue,
            }
        }
    }
}

struct TimeoutFuture<F> {
    inner: F,
    timeout: Delay,

    cancelled: bool,
}

impl<F> Future for TimeoutFuture<F>
where
    F: Future + Unpin,
{
    type Output = Result<F::Output, TimeoutError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.cancelled {
            return Poll::Ready(Err(TimeoutError::Cancelled));
        }

        if self.timeout.poll_unpin(cx).is_ready() {
            return Poll::Ready(Err(TimeoutError::Timeout));
        }

        self.inner.poll_unpin(cx).map(Ok)
    }
}

enum TimeoutError {
    Timeout,
    Cancelled,
}

struct TaggedFuture<T, F> {
    tag: T,
    inner: F,
}

impl<T, F> Future for TaggedFuture<T, F>
where
    T: Clone + Unpin,
    F: Future + Unpin,
{
    type Output = (T, F::Output);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let output = futures_util::ready!(self.inner.poll_unpin(cx));

        Poll::Ready((self.tag.clone(), output))
    }
}

#[cfg(test)]
mod tests {
    use futures::channel::oneshot;
    use futures_util::task::noop_waker_ref;
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
            Err(PushError::Replaced(_))
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

    #[test]
    fn resources_of_removed_future_are_cleaned_up() {
        let mut futures = FuturesMap::new(Duration::from_millis(100), 1);

        let _ = futures.try_push("ID", pending::<()>());
        futures.remove("ID");

        let poll = futures.poll_unpin(&mut Context::from_waker(noop_waker_ref()));
        assert!(poll.is_pending());

        assert_eq!(futures.len(), 0);
    }

    #[tokio::test]
    async fn replaced_pending_future_is_polled() {
        let mut streams = FuturesMap::new(Duration::from_millis(100), 3);

        let (_tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();

        let _ = streams.try_push("ID1", rx1);
        let _ = streams.try_push("ID2", rx2);

        let _ = tx2.send(2);
        let (id, res) = poll_fn(|cx| streams.poll_unpin(cx)).await;
        assert_eq!(id, "ID2");
        assert_eq!(res.unwrap().unwrap(), 2);

        let (new_tx1, new_rx1) = oneshot::channel();
        let replaced = streams.try_push("ID1", new_rx1);
        assert!(matches!(replaced.unwrap_err(), PushError::Replaced(_)));

        let _ = new_tx1.send(4);
        let (id, res) = poll_fn(|cx| streams.poll_unpin(cx)).await;

        assert_eq!(id, "ID1");
        assert_eq!(res.unwrap().unwrap(), 4);
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
