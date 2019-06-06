use futures::prelude::*;
use bytes::BytesMut;
use crate::error::TlsError;

pub struct DecoderMiddleware<S> {
    pub raw_stream: S,
}

impl<S> Sink for DecoderMiddleware<S>
where
    S: Sink,
{
    type SinkItem = S::SinkItem;
    type SinkError = S::SinkError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.raw_stream.start_send(item)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.raw_stream.poll_complete()
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.raw_stream.close()
    }
}

impl<S> Stream for DecoderMiddleware<S>
where
    S: Stream,
{
    type Item = S::Item;
    type Error = S::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.raw_stream.poll()
    }
}