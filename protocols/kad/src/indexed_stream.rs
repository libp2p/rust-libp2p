use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct IndexedStream<I, S> {
    index: I,
    stream: S,
}

impl<I, S> IndexedStream<I, S> {
    pub fn new(index: I, stream: S) -> Self {
        Self { index, stream }
    }

    pub fn index(&self) -> &I {
        &self.index
    }

    pub fn stream_pin_mut(&mut self) -> Pin<&mut S> {
        // Safety: We never expose an unpinned reference.
        unsafe { Pin::new_unchecked(&mut self.stream) }
    }
}

impl<I, S> Stream for IndexedStream<I, S>
where
    I: Clone,
    S: Stream,
{
    type Item = (I, S::Item);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Safety: We never expose an unpinned reference.
        let index = self.index.clone();

        match futures::ready!(unsafe {
            Pin::new_unchecked(&mut self.get_unchecked_mut().stream).poll_next(cx)
        }) {
            None => Poll::Ready(None),
            Some(item) => Poll::Ready(Some((index, item))),
        }
    }
}
