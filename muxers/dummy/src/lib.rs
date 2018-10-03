extern crate futures;
extern crate libp2p_core as core;

use std::io::Error as IoError;
use core::muxing::StreamMuxer;
use futures::prelude::*;

pub struct DummySubstream {}

pub struct OutboundSubstream {}

#[derive(Debug, PartialEq)]
pub enum DummyConnectionState {
    Substream,
    Pending,
    Open,
    Closed,
}
#[derive(Debug)]
pub struct DummyConnection {
    pub state: DummyConnectionState
}

#[derive(Debug)]
pub struct DummyMuxer{
    pub in_connection: DummyConnection,
    pub out_connection: DummyConnection,
}

impl DummyMuxer {
    pub fn new() -> Self {
        DummyMuxer {
            in_connection: DummyConnection{ state: DummyConnectionState::Pending },
            out_connection: DummyConnection{ state: DummyConnectionState::Closed },
        }
    }
    pub fn set_inbound_connection_state(&mut self, state: DummyConnectionState) {
        self.in_connection.state = state
    }
    pub fn set_outbound_connection_state(&mut self, state: DummyConnectionState) {
        self.out_connection.state = state
    }
}

impl StreamMuxer for DummyMuxer {
    type Substream = DummySubstream;
    type OutboundSubstream = OutboundSubstream;
    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError> {
        match self.in_connection.state {
            DummyConnectionState::Pending => Ok(Async::NotReady),
            DummyConnectionState::Closed => Ok(Async::Ready(None)),
            _ => unimplemented!()
        }
    }
    #[allow(unused_variables)]
    fn open_outbound(&self) -> Self::OutboundSubstream { Self::OutboundSubstream{} }
    #[allow(unused_variables)]
    fn poll_outbound(&self, substream: &mut Self::OutboundSubstream) -> Poll<Option<Self::Substream>, IoError> {
        match self.out_connection.state {
            DummyConnectionState::Pending => Ok(Async::NotReady),
            DummyConnectionState::Closed => Ok(Async::Ready(None)),
            _ => unimplemented!()
        }
    }
    #[allow(unused_variables)]
    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {}
    #[allow(unused_variables)]
    fn read_substream(&self, substream: &mut Self::Substream, buf: &mut [u8]) -> Result<usize, IoError> { Ok(1) }
    #[allow(unused_variables)]
    fn write_substream(&self, substream: &mut Self::Substream, buf: &[u8]) -> Result<usize, IoError> { Ok(1) }
    #[allow(unused_variables)]
    fn flush_substream(&self, substream: &mut Self::Substream) -> Result<(), IoError> { Ok(()) }
    #[allow(unused_variables)]
    fn shutdown_substream(&self, substream: &mut Self::Substream) -> Poll<(), IoError> { Ok(Async::Ready(())) }
    #[allow(unused_variables)]
    fn destroy_substream(&self, substream: Self::Substream) {}
    fn close_inbound(&self) {}
    fn close_outbound(&self) {}
}
