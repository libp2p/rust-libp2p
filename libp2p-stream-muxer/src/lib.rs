extern crate tokio_io;
extern crate futures;

use futures::stream::Stream;
use tokio_io::{AsyncRead, AsyncWrite};

pub trait StreamMuxer {
    type Substream: AsyncRead + AsyncWrite;
    type InboundSubstreams: Stream<Item = Self::Substream>;
    type OutboundSubstreams: Stream<Item = Self::Substream>;

    fn inbound(&mut self) -> Self::InboundSubstreams;
    fn outbound(&mut self) -> Self::OutboundSubstreams;
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
