use std::future::Future;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_timer::Delay;
use futures_util::future::{select, BoxFuture, Either};
use futures_util::stream::FuturesUnordered;
use futures_util::{ready, FutureExt, StreamExt};

/// Represents a set of (Worker)-[Future]s.
///
/// This wraps [FuturesUnordered] but bounds it by time and size.
/// In other words, each worker must finish within the specified time and the set never outgrows its capacity.
pub struct WorkerFutures<K, O> {
    timeout: Duration,
    capacity: usize,
    inner: FuturesUnordered<BoxFuture<'static, (K, Result<O, Timeout>)>>,

    empty_waker: Option<Waker>,
    full_waker: Option<Waker>,
}

impl<K, O> WorkerFutures<K, O> {
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

impl<K, O> WorkerFutures<K, O>
where
    K: Send + 'static,
{
    pub fn try_push<F>(&mut self, key: K, worker: F) -> Option<F>
    where
        F: Future<Output = O> + Send + 'static + Unpin,
    {
        if self.inner.len() >= self.capacity {
            return Some(worker);
        }
        let timeout = Delay::new(self.timeout);

        self.inner.push(
            async move {
                match select(worker, timeout).await {
                    Either::Left((out, _)) => (key, Ok(out)),
                    Either::Right(((), _)) => (key, Err(Timeout::new())),
                }
            }
            .boxed(),
        );

        None
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

    pub fn poll_unpin(&mut self, cx: &mut Context<'_>) -> Poll<(K, Result<O, Timeout>)> {
        match ready!(self.inner.poll_next_unpin(cx)) {
            None => {
                self.empty_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Some(result) => {
                if let Some(waker) = self.full_waker.take() {
                    waker.wake();
                }

                Poll::Ready(result)
            }
        }
    }
}

pub struct Timeout {
    _priv: (),
}

impl Timeout {
    fn new() -> Self {
        Self { _priv: () }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{pending, poll_fn, ready};

    #[test]
    fn cannot_push_more_than_capacity_tasks() {
        let mut workers = WorkerFutures::new(Duration::from_secs(10), 1);

        assert!(workers.try_push((), ready(())).is_none());
        assert!(workers.try_push((), ready(())).is_some());
    }

    #[tokio::test]
    async fn workers_timeout() {
        let mut workers = WorkerFutures::new(Duration::from_millis(100), 1);

        let _ = workers.try_push((), pending::<()>());
        Delay::new(Duration::from_millis(150)).await;
        let (_, result) = poll_fn(|cx| workers.poll_unpin(cx)).await;

        assert!(result.is_err())
    }
}
