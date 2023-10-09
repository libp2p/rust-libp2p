use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures_timer::Delay;
use futures_util::stream::{BoxStream, SelectAll};
use futures_util::{FutureExt, Stream, StreamExt};

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

    reported_none: bool,
}

impl<K, S> TaggedStream<K, S> {
    fn new(key: K, inner: S) -> Self {
        Self {
            key,
            inner,
            reported_none: false,
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
        if self.reported_none {
            return Poll::Ready(None);
        }

        match futures_util::ready!(self.inner.poll_next_unpin(cx)) {
            Some(item) => Poll::Ready(Some((self.key.clone(), Some(item)))),
            None => {
                self.reported_none = true;

                Poll::Ready(Some((self.key.clone(), None)))
            }
        }
    }
}
