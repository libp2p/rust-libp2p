use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_timer::Delay;
use futures_util::stream::{BoxStream, SelectAll};
use futures_util::{stream, FutureExt, Stream, StreamExt};

use crate::{PushError, Timeout};

/// Represents a map of [`Stream`]s.
///
/// Each stream must finish within the specified time and the map never outgrows its capacity.
pub struct StreamMap<ID, O> {
    timeout: Duration,
    capacity: usize,
    inner: SelectAll<TaggedStream<ID, TimeoutStream<BoxStream<'static, O>>>>,
    empty_waker: Option<Waker>,
    full_waker: Option<Waker>,
}

impl<ID, O> StreamMap<ID, O>
where
    ID: Clone + Unpin,
{
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

impl<ID, O> StreamMap<ID, O>
where
    ID: Clone + PartialEq + Send + Unpin + 'static,
    O: Send + 'static,
{
    /// Push a stream into the map.
    pub fn try_push<F>(&mut self, id: ID, stream: F) -> Result<(), PushError<BoxStream<O>>>
    where
        F: Stream<Item = O> + Send + 'static,
    {
        if self.inner.len() >= self.capacity {
            return Err(PushError::BeyondCapacity(stream.boxed()));
        }

        if let Some(waker) = self.empty_waker.take() {
            waker.wake();
        }

        match self.inner.iter_mut().find(|tagged| tagged.key == id) {
            None => {
                self.inner.push(TaggedStream::new(
                    id,
                    TimeoutStream {
                        inner: stream.boxed(),
                        timeout: Delay::new(self.timeout),
                    },
                ));

                Ok(())
            }
            Some(existing) => {
                let old = mem::replace(
                    &mut existing.inner,
                    TimeoutStream {
                        inner: stream.boxed(),
                        timeout: Delay::new(self.timeout),
                    },
                );

                Err(PushError::Replaced(old.inner))
            }
        }
    }

    pub fn remove(&mut self, id: ID) -> Option<BoxStream<O>> {
        let tagged = self.inner.iter_mut().find(|s| s.key == id)?;

        let inner = mem::replace(&mut tagged.inner.inner, stream::pending().boxed());
        tagged.exhausted = true; // Setting this will emit `None` on the next poll and ensure `SelectAll` cleans up the resources.

        Some(inner)
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

    pub fn poll_next_unpin(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<(ID, Option<Result<O, Timeout>>)> {
        match futures_util::ready!(self.inner.poll_next_unpin(cx)) {
            None => {
                self.empty_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Some((id, Some(Ok(output)))) => Poll::Ready((id, Some(Ok(output)))),
            Some((id, Some(Err(())))) => Poll::Ready((id, Some(Err(Timeout::new(self.timeout))))),
            Some((id, None)) => Poll::Ready((id, None)),
        }
    }
}

struct TimeoutStream<F> {
    inner: F,
    timeout: Delay,
}

impl<F> Stream for TimeoutStream<F>
where
    F: Stream + Unpin,
{
    type Item = Result<F::Item, ()>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.timeout.poll_unpin(cx).is_ready() {
            return Poll::Ready(Some(Err(())));
        }

        self.inner.poll_next_unpin(cx).map(|a| a.map(Ok))
    }
}

struct TaggedStream<K, S> {
    key: K,
    inner: S,

    exhausted: bool,
}

impl<K, S> TaggedStream<K, S> {
    fn new(key: K, inner: S) -> Self {
        Self {
            key,
            inner,
            exhausted: false,
        }
    }
}

impl<K, S> Stream for TaggedStream<K, S>
where
    K: Clone + Unpin,
    S: Stream + Unpin,
{
    type Item = (K, Option<S::Item>);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.exhausted {
            return Poll::Ready(None);
        }

        match futures_util::ready!(self.inner.poll_next_unpin(cx)) {
            Some(item) => Poll::Ready(Some((self.key.clone(), Some(item)))),
            None => {
                self.exhausted = true;

                Poll::Ready(Some((self.key.clone(), None)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use futures_util::stream::{once, pending};
    use std::future::{poll_fn, ready, Future};
    use std::pin::Pin;
    use std::time::Instant;

    use super::*;

    #[test]
    fn cannot_push_more_than_capacity_tasks() {
        let mut streams = StreamMap::new(Duration::from_secs(10), 1);

        assert!(streams.try_push("ID_1", once(ready(()))).is_ok());
        matches!(
            streams.try_push("ID_2", once(ready(()))),
            Err(PushError::BeyondCapacity(_))
        );
    }

    #[test]
    fn cannot_push_the_same_id_few_times() {
        let mut streams = StreamMap::new(Duration::from_secs(10), 5);

        assert!(streams.try_push("ID", once(ready(()))).is_ok());
        matches!(
            streams.try_push("ID", once(ready(()))),
            Err(PushError::Replaced(_))
        );
    }

    #[tokio::test]
    async fn streams_timeout() {
        let mut streams = StreamMap::new(Duration::from_millis(100), 1);

        let _ = streams.try_push("ID", pending::<()>());
        Delay::new(Duration::from_millis(150)).await;
        let (_, result) = poll_fn(|cx| streams.poll_next_unpin(cx)).await;

        assert!(result.unwrap().is_err())
    }

    #[test]
    fn removing_stream() {
        let mut streams = StreamMap::new(Duration::from_millis(100), 1);

        let _ = streams.try_push("ID", stream::once(ready(())));

        {
            let cancelled_stream = streams.remove("ID");
            assert!(cancelled_stream.is_some());
        }

        let poll = streams.poll_next_unpin(&mut Context::from_waker(
            futures_util::task::noop_waker_ref(),
        ));

        assert!(poll.is_pending());
        assert_eq!(
            streams.inner.len(),
            0,
            "resources of cancelled streams are cleaned up properly"
        );
    }

    // Each stream emits 1 item with delay, `Task` only has a capacity of 1, meaning they must be processed in sequence.
    // We stop after NUM_STREAMS tasks, meaning the overall execution must at least take DELAY * NUM_FUTURES.
    #[tokio::test]
    async fn backpressure() {
        const DELAY: Duration = Duration::from_millis(100);
        const NUM_STREAMS: u32 = 10;

        let start = Instant::now();
        Task::new(DELAY, NUM_STREAMS, 1).await;
        let duration = start.elapsed();

        assert!(duration >= DELAY * NUM_STREAMS);
    }

    struct Task {
        item_delay: Duration,
        num_streams: usize,
        num_processed: usize,
        inner: StreamMap<u8, ()>,
    }

    impl Task {
        fn new(item_delay: Duration, num_streams: u32, capacity: usize) -> Self {
            Self {
                item_delay,
                num_streams: num_streams as usize,
                num_processed: 0,
                inner: StreamMap::new(Duration::from_secs(60), capacity),
            }
        }
    }

    impl Future for Task {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();

            while this.num_processed < this.num_streams {
                match this.inner.poll_next_unpin(cx) {
                    Poll::Ready((_, Some(result))) => {
                        if result.is_err() {
                            panic!("Timeout is great than item delay")
                        }

                        this.num_processed += 1;
                        continue;
                    }
                    Poll::Ready((_, None)) => {
                        continue;
                    }
                    _ => {}
                }

                if let Poll::Ready(()) = this.inner.poll_ready_unpin(cx) {
                    // We push the constant ID to prove that user can use the same ID if the stream was finished
                    let maybe_future = this.inner.try_push(1u8, once(Delay::new(this.item_delay)));
                    assert!(maybe_future.is_ok(), "we polled for readiness");

                    continue;
                }

                return Poll::Pending;
            }

            Poll::Ready(())
        }
    }
}
