use futures::prelude::*;
use bytes::BytesMut;

pub struct EncoderMiddleware<S> {
    pub raw_sink: S,
}

impl<S> Sink for EncoderMiddleware<S>
where
    S: Sink,
{
    type SinkItem = S::SinkItem;
    type SinkError = S::SinkError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.raw_sink.start_send(item)
    }

    fn poll_complete(&mut self) -> Result<Async<()>, Self::SinkError> {
        self.raw_sink.poll_complete()
    }

    fn close(&mut self) -> Result<Async<()>, Self::SinkError> {
        self.raw_sink.close()
    }
}

impl<S> Stream for EncoderMiddleware<S>
where
    S: Stream,
{
    type Item = S::Item;
    type Error = S::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.raw_sink.poll()
    }
}
