// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! `DummyMuxer` is a `StreamMuxer` to be used in tests. It implements a bare-bones
//! version of the trait along with a way to setup the muxer to behave in the
//! desired way when testing other components.

use futures::prelude::*;
use crate::muxing::StreamMuxer;
use std::io::Error as IoError;

/// Substream type
#[derive(Debug)]
pub struct DummySubstream {}

/// OutboundSubstream type
#[derive(Debug)]
pub struct DummyOutboundSubstream {}

/// Control the muxer state by setting the "connection" state as to set up a mock
/// muxer for higher level components.
#[derive(Debug, PartialEq, Clone)]
pub enum DummyConnectionState {
    Pending, // use this to trigger the Async::NotReady code path
    Opened,  // use this to trigger the Async::Ready(_) code path
}
#[derive(Debug, PartialEq, Clone)]
struct DummyConnection {
    state: DummyConnectionState,
}

/// `DummyMuxer` implements `StreamMuxer` and methods to control its behaviour when used in tests
#[derive(Debug, PartialEq, Clone)]
pub struct DummyMuxer{
    in_connection: DummyConnection,
    out_connection: DummyConnection,
}

impl DummyMuxer {
    /// Create a new `DummyMuxer` where the inbound substream is set to `Pending`
    /// and the (single) outbound substream to `Pending`.
    pub fn new() -> Self {
        DummyMuxer {
            in_connection: DummyConnection {
                state: DummyConnectionState::Pending,
            },
            out_connection: DummyConnection {
                state: DummyConnectionState::Pending,
            },
        }
    }
    /// Set the muxer state inbound "connection" state
    pub fn set_inbound_connection_state(&mut self, state: DummyConnectionState) {
        self.in_connection.state = state
    }
    /// Set the muxer state outbound "connection" state
    pub fn set_outbound_connection_state(&mut self, state: DummyConnectionState) {
        self.out_connection.state = state
    }
}

impl StreamMuxer for DummyMuxer {
    type Substream = DummySubstream;
    type OutboundSubstream = DummyOutboundSubstream;
    type Error = IoError;
    fn poll_inbound(&self) -> Poll<Self::Substream, IoError> {
        match self.in_connection.state {
            DummyConnectionState::Pending => Ok(Async::NotReady),
            DummyConnectionState::Opened => Ok(Async::Ready(Self::Substream {})),
        }
    }
    fn open_outbound(&self) -> Self::OutboundSubstream {
        Self::OutboundSubstream {}
    }
    fn poll_outbound(
        &self,
        _substream: &mut Self::OutboundSubstream,
    ) -> Poll<Self::Substream, IoError> {
        match self.out_connection.state {
            DummyConnectionState::Pending => Ok(Async::NotReady),
            DummyConnectionState::Opened => Ok(Async::Ready(Self::Substream {})),
        }
    }
    fn destroy_outbound(&self, _: Self::OutboundSubstream) {}
    fn read_substream(&self, _: &mut Self::Substream, _buf: &mut [u8]) -> Poll<usize, IoError> {
        unreachable!()
    }
    fn write_substream(&self, _: &mut Self::Substream, _buf: &[u8]) -> Poll<usize, IoError> {
        unreachable!()
    }
    fn flush_substream(&self, _: &mut Self::Substream) -> Poll<(), IoError> {
        unreachable!()
    }
    fn shutdown_substream(&self, _: &mut Self::Substream) -> Poll<(), IoError> {
        unreachable!()
    }
    fn destroy_substream(&self, _: Self::Substream) {}
    fn is_remote_acknowledged(&self) -> bool { true }
    fn close(&self) -> Poll<(), IoError> {
        Ok(Async::Ready(()))
    }
    fn flush_all(&self) -> Poll<(), IoError> {
        Ok(Async::Ready(()))
    }
}
