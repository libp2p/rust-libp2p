use futures::{FutureExt, Stream, StreamExt};
use futures_timer::Delay;
use std::fmt;
use std::fmt::Formatter;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

/// A container type for polling multiple streams concurrently.
///
/// Each stream is raced against a timeout specified upon construction of [`TimedStreams`].
///
/// [`TimedStreams`] implements [`Stream`] but will never emit [`None`].
/// Instead, a waker is registered internally and woken once the next task gets pushed.
pub struct TimedStreams<S> {
    inner: futures::stream::SelectAll<StreamWrapper<S>>,
    capacity: usize,
    timeout: Duration,
    empty_waker: Option<Waker>,
}

impl<S> TimedStreams<S>
where
    S: Stream + Unpin,
{
    pub fn new(capacity: usize, timeout: Duration) -> Self {
        Self {
            inner: futures::stream::SelectAll::default(),
            capacity,
            timeout,
            empty_waker: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn try_push(&mut self, stream: S) -> Result<(), CapacityExceeded<S>> {
        if self.inner.len() >= self.capacity {
            return Err(CapacityExceeded { stream });
        }

        self.inner.push(StreamWrapper {
            inner: stream,
            timer: Delay::new(self.timeout),
            complete: false,
        });

        if let Some(waker) = self.empty_waker.take() {
            waker.wake();
        }

        Ok(())
    }
}

impl<S> Stream for TimedStreams<S>
where
    S: Stream + Unpin,
{
    type Item = Result<S::Item, Timeout>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match std::task::ready!(self.inner.poll_next_unpin(cx)) {
            Some(Ok(item)) => Poll::Ready(Some(Ok(item))),
            Some(Err(InternalTimeout {})) => Poll::Ready(Some(Err(Timeout {
                duration: self.timeout,
            }))),
            None => {
                self.empty_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

// TODO impl error
#[derive(Debug)]
pub struct Timeout {
    duration: Duration,
}

impl fmt::Display for Timeout {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "stream failed to complete within {}s",
            self.duration.as_secs()
        )
    }
}

impl std::error::Error for Timeout {}

pub struct CapacityExceeded<S> {
    pub stream: S,
}

impl<S> fmt::Debug for CapacityExceeded<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapacityExceeded").finish_non_exhaustive()
    }
}

impl<S> fmt::Display for CapacityExceeded<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "no capacity for stream")
    }
}

impl<S> std::error::Error for CapacityExceeded<S> {}

struct StreamWrapper<S> {
    inner: S,
    timer: Delay,
    complete: bool,
}

/// Internal timeout error without any metadata
struct InternalTimeout {}

impl<S> Stream for StreamWrapper<S>
where
    S: Stream + Unpin,
{
    type Item = Result<S::Item, InternalTimeout>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.complete {
            return Poll::Ready(None);
        }

        if self.timer.poll_unpin(cx).is_ready() {
            self.complete = true;
            return Poll::Ready(Some(Err(InternalTimeout {})));
        }

        let item = std::task::ready!(self.inner.poll_next_unpin(cx)).map(Ok);

        if item.is_none() {
            self.complete = true
        }

        Poll::Ready(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn stream_that_exceeds_timeout_errors() {
        let mut streams = TimedStreams::new(10, Duration::from_millis(100));

        streams.try_push(futures::stream::pending::<()>()).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert!((streams.next().await).unwrap().is_err())
    }

    #[tokio::test]
    async fn stream_that_finishes_in_time_does_not_occur_error() {
        let mut streams = TimedStreams::new(10, Duration::from_secs(1));

        streams
            .try_push(futures::stream::once(futures::future::ready(())))
            .unwrap();

        streams.next().await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn finished_streams_are_tidied_up() {
        let mut streams = TimedStreams::new(10, Duration::from_secs(1));

        streams
            .try_push(futures::stream::once(futures::future::ready(())))
            .unwrap();

        streams.next().await.unwrap().unwrap();

        streams.next().now_or_never(); // allow for clean-up
        assert_eq!(streams.len(), 0)
    }
}
